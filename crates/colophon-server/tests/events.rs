//! Live change hints end to end (D-108, migration 0044): `{P}/events` answers a signed-in
//! stream with `hello`, a committed write — here or on another server — arrives as `change`,
//! projects stay isolated, keep-alives hold an idle stream, and a membership check that stops
//! passing ends the stream. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::time::{Duration, Instant};

use common::{
    PORT, Receiver, Server, Session, app, call, create_article, destination, finished_job, join,
    key, project, publish_article, sign_in,
};
use reqwest::{Method, StatusCode, header};

/// The `{P}/events` of `project`'s production environment.
fn events(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production/events")
}

/// One open stream: the response and the bytes read so far.
struct Sse {
    response: reqwest::Response,
    buffered: String,
}

/// What a wait on the stream found.
enum Next {
    /// A complete SSE frame (`event:`/`data:` lines, or a comment).
    Frame(String),
    /// The response body ended.
    Ended,
    /// Nothing complete arrived within the wait.
    Quiet,
}

impl Sse {
    /// `GET {path}` on `origin` with `headers`; the SSE response with headers checked — they
    /// come first, well inside the request timeout, with the body still streaming.
    async fn open(origin: &str, path: &str, headers: &[(&str, String)]) -> Self {
        let mut request = app().get(format!("{origin}{path}"));
        for (name, value) in headers {
            request = request.header(*name, value);
        }
        let started = Instant::now();
        let response = request.send().await.expect("events request");
        assert_eq!(
            response.status(),
            StatusCode::OK,
            "{}",
            started.elapsed().as_secs()
        );
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        assert!(
            content_type.starts_with("text/event-stream"),
            "{content_type}"
        );
        assert_eq!(
            response
                .headers()
                .get("x-accel-buffering")
                .and_then(|value| value.to_str().ok()),
            Some("no"),
            "buffering proxies are asked not to hold events back"
        );
        Self {
            response,
            buffered: String::new(),
        }
    }

    /// A signed-in stream on the test origin.
    async fn subscribe(session: &Session, project: &str) -> Self {
        Self::open(
            common::ORIGIN,
            &events(project),
            &[(header::COOKIE.as_str(), session.cookie.clone())],
        )
        .await
    }

    /// The next complete frame, `Ended` when the server closes the body, or `Quiet` after
    /// `within`.
    async fn next(&mut self, within: Duration) -> Next {
        let deadline = Instant::now() + within;
        loop {
            if let Some(end) = self.buffered.find("\n\n") {
                return Next::Frame(self.buffered.drain(..end + 2).collect());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Next::Quiet;
            }
            match tokio::time::timeout(left, self.response.chunk()).await {
                Err(_elapsed) => return Next::Quiet,
                Ok(Err(error)) => panic!("the stream failed: {error}"),
                Ok(Ok(None)) => return Next::Ended,
                Ok(Ok(Some(bytes))) => {
                    self.buffered.push_str(&String::from_utf8_lossy(&bytes));
                }
            }
        }
    }

    /// The next `change` or `resync` frame's data, skipping comments and `hello` once seen.
    /// Fails on `Ended`/`Quiet` after `within`.
    async fn change(&mut self, within: Duration) -> String {
        let deadline = Instant::now() + within;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            assert!(!left.is_zero(), "no change frame within {within:?}");
            match self.next(left).await {
                Next::Frame(frame) if frame.starts_with("event: change") => {
                    let data = frame
                        .lines()
                        .find_map(|line| line.strip_prefix("data: "))
                        .expect("a data line")
                        .to_owned();
                    return data;
                }
                Next::Frame(_) => {}
                Next::Ended => panic!("the stream ended before a change"),
                Next::Quiet => panic!("no change frame within {within:?}"),
            }
        }
    }
}

#[tokio::test]
#[ignore = "needs the stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn a_committed_write_reaches_a_signed_in_stream_and_other_projects_hear_nothing() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let other = common::project(&owner).await;

    // Wrong credentials and wrong projects are refused before the stream exists.
    let anonymous = app()
        .get(format!("{}{}", common::ORIGIN, events(&project)))
        .send()
        .await
        .expect("request");
    assert_eq!(anonymous.status(), StatusCode::UNAUTHORIZED);
    let stranger = sign_in("dev-editor").await;
    assert_eq!(
        call(&stranger, Method::GET, &events(&project), None, &[])
            .await
            .expect("request")
            .status,
        StatusCode::NOT_FOUND,
        "a member of nothing learns nothing of the project"
    );
    assert_eq!(
        call(
            &owner,
            Method::GET,
            &events(&uuid::Uuid::now_v7().to_string()),
            None,
            &[]
        )
        .await
        .expect("request")
        .status,
        StatusCode::NOT_FOUND
    );

    // `Last-Event-ID` is accepted and ignored: there is no replay to resume.
    let mut stream = Sse::open(
        common::ORIGIN,
        &events(&project),
        &[
            (header::COOKIE.as_str(), owner.cookie.clone()),
            ("last-event-id", "42".to_owned()),
        ],
    )
    .await;
    match stream.next(Duration::from_secs(5)).await {
        Next::Frame(frame) => assert!(frame.starts_with("event: hello"), "{frame}"),
        _ => panic!("hello comes first"),
    }

    // A write in another project stays in that project: nothing arrives here for it.
    create_article(&owner, &other, "Elsewhere").await;
    match stream.next(Duration::from_secs(1)).await {
        Next::Quiet => {}
        Next::Frame(frame) => panic!("another project's write leaked: {frame}"),
        Next::Ended => panic!("the stream ended"),
    }

    // A write in this one arrives as a hint naming the kind and collection, nothing more.
    create_article(&owner, &project, "Live").await;
    let data = stream.change(Duration::from_secs(5)).await;
    let hint: serde_json::Value = serde_json::from_str(&data).expect("the hint is JSON");
    assert_eq!(
        (hint["k"].as_str(), hint["c"].as_str()),
        (Some("entries"), Some("articles")),
        "{hint}"
    );
    assert!(hint["e"].as_str().is_some(), "scoped to the environment");
    assert_eq!(
        hint.as_object().expect("an object").len(),
        3,
        "just k, c, e"
    );
}

#[tokio::test]
#[ignore = "needs the stack and migrations"]
async fn a_project_api_token_listens_too() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(
            &serde_json::json!({"name": "Site build", "kind": "read-only", "expires_in_days": 30}),
        ),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"].as_str().expect("the secret");

    let mut stream = Sse::open(
        common::ORIGIN,
        &events(&project),
        &[(header::AUTHORIZATION.as_str(), format!("Bearer {secret}"))],
    )
    .await;
    match stream.next(Duration::from_secs(5)).await {
        Next::Frame(frame) => assert!(frame.starts_with("event: hello"), "{frame}"),
        _ => panic!("hello comes first"),
    }
}

#[tokio::test]
#[ignore = "needs the stack and migrations"]
async fn a_write_on_a_second_server_reaches_this_ones_streams() {
    let _one = Server::start().await;
    let _two = Server::start_on(PORT + 50, &[]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let mut stream = Sse::subscribe(&owner, &project).await;
    match stream.next(Duration::from_secs(5)).await {
        Next::Frame(frame) => assert!(frame.starts_with("event: hello"), "{frame}"),
        _ => panic!("hello comes first"),
    }

    // The write goes to the second server; the hint travels through Postgres to this one.
    let write = app()
        .post(format!(
            "http://127.0.0.1:{}{}",
            PORT + 50,
            common::entries(&project)
        ))
        .header(header::COOKIE, &owner.cookie)
        .header("x-csrf-token", &owner.csrf)
        .header(header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", key())
        .json(&common::article("Across servers", "text"))
        .send()
        .await
        .expect("request");
    assert_eq!(write.status(), StatusCode::CREATED);
    let data = stream.change(Duration::from_secs(5)).await;
    assert!(data.contains(r#""k":"entries""#), "{data}");
}

#[tokio::test]
#[ignore = "needs the stack and migrations"]
async fn publishing_and_the_workers_job_updates_reach_the_stream() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let receiver = Receiver::start(&[200]).await;
    destination(&owner, &project, &receiver.url).await;
    let mut stream = Sse::subscribe(&owner, &project).await;
    match stream.next(Duration::from_secs(5)).await {
        Next::Frame(frame) => assert!(frame.starts_with("event: hello"), "{frame}"),
        _ => panic!("hello comes first"),
    }

    // The publish's own commit and every worker-side job update are `releases` hints.
    let generation = publish_article(&owner, &project, "Live").await;
    let job = finished_job(&owner, &project, generation, 15).await;
    assert_eq!(job["status"].as_str(), Some("succeeded"));
    let mut releases = 0;
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline && releases < 2 {
        match stream.next(Duration::from_millis(300)).await {
            Next::Frame(frame) if frame.contains(r#""k":"releases""#) => releases += 1,
            Next::Ended => panic!("the stream ended"),
            _ => {}
        }
    }
    assert!(
        releases >= 2,
        "the insert plus the worker's updates each hinted: {releases}"
    );
}

#[tokio::test]
#[ignore = "needs the stack and migrations"]
async fn an_idle_stream_stays_open_past_ten_seconds() {
    let _server = Server::start_with(&[("COLOPHON_EVENTS_KEEPALIVE_MS", "2000")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let mut stream = Sse::subscribe(&owner, &project).await;
    match stream.next(Duration::from_secs(5)).await {
        Next::Frame(frame) => assert!(frame.starts_with("event: hello"), "{frame}"),
        _ => panic!("hello comes first"),
    }

    // A keep-alive comment every two seconds; the stream still answers past ten.
    let mut comments = 0;
    let deadline = Instant::now() + Duration::from_secs(11);
    while Instant::now() < deadline {
        match stream.next(Duration::from_millis(500)).await {
            Next::Frame(frame) if frame.starts_with(':') => comments += 1,
            Next::Ended => panic!("the stream ended early"),
            _ => {}
        }
    }
    assert!(comments >= 4, "keep-alives held the stream: {comments}");
    create_article(&owner, &project, "After ten seconds").await;
    let data = stream.change(Duration::from_secs(5)).await;
    assert!(data.contains(r#""k":"entries""#), "{data}");
}

#[tokio::test]
#[ignore = "needs the stack and migrations"]
async fn a_removed_members_stream_ends_at_the_next_check() {
    let _server = Server::start_with(&[("COLOPHON_EVENTS_REVALIDATE_MS", "400")]).await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &guest, &project, "editor").await;
    let mut stream = Sse::subscribe(&guest, &project).await;
    match stream.next(Duration::from_secs(5)).await {
        Next::Frame(frame) => assert!(frame.starts_with("event: hello"), "{frame}"),
        _ => panic!("hello comes first"),
    }

    // The owner removes the guest; the next re-check ends the stream.
    let members = call(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
        &[],
    )
    .await
    .expect("request");
    let guest_id = members.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .find_map(|member| {
            (member["role"].as_str() == Some("editor"))
                .then(|| member["identity_id"].as_str().expect("an id").to_owned())
        })
        .expect("the guest's membership");
    let removed = call(
        &owner,
        Method::DELETE,
        &format!("/api/v1/projects/{project}/members/{guest_id}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(removed.status, StatusCode::NO_CONTENT, "{}", removed.body);

    // The removal itself hints; the stream still ends within a few checks.
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match stream.next(Duration::from_secs(2)).await {
            Next::Ended => break,
            Next::Quiet | Next::Frame(_) => {
                assert!(Instant::now() < deadline, "the stream never ended");
            }
        }
    }
}
