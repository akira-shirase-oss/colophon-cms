//! The delivery cache against the real server, Postgres and Valkey (docs/modules/06-delivery.md;
//! D-048): answers are cached by generation, a new release shows at once (F06), a hot key is read
//! from Postgres once however many ask (L04), answers survive Valkey going away and caching
//! resumes when it's back (F08), and two projects never share an answer. Needs the stack and
//! `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::{
    collections::BTreeMap,
    path::PathBuf,
    process::{Command, Stdio},
    time::{Duration, Instant},
};

use common::{ORIGIN, Server, app, project, publish_article, sign_in};
use reqwest::{StatusCode, header};
use serde_json::Value;

const LATEST: &str = "public, max-age=0, must-revalidate, s-maxage=30";
const PINNED: &str = "public, max-age=0, must-revalidate, s-maxage=300";

/// One delivery answer, with the headers the cache sets.
struct Read {
    status: StatusCode,
    served: String,
    generation: String,
    etag: String,
    cache_control: String,
    tags: String,
    body: Value,
}

async fn read(path: &str, if_none_match: Option<&str>) -> Read {
    let mut request = app().get(format!("{ORIGIN}{path}"));
    if let Some(etag) = if_none_match {
        request = request.header(header::IF_NONE_MATCH, etag);
    }
    let response = request.send().await.expect("request");
    let text = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let (status, served, generation, etag, cache_control, tags) = (
        response.status(),
        text("colophon-cache"),
        text("colophon-generation"),
        text("etag"),
        text("cache-control"),
        text("cache-tag"),
    );
    let bytes = response.bytes().await.expect("body");
    let body = serde_json::from_slice(&bytes).unwrap_or(Value::Null);
    Read {
        status,
        served,
        generation,
        etag,
        cache_control,
        tags,
        body,
    }
}

fn titles(body: &Value) -> Vec<String> {
    body["data"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| item["title"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn answers_are_cached_by_generation_and_a_new_release_shows_at_once() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    assert_eq!(publish_article(&owner, &id, "First").await, 1);
    let list = format!("/content/v1/{id}/production/articles");

    let first = read(&list, None).await;
    assert_eq!(first.status, StatusCode::OK);
    assert_eq!(first.served, "miss");
    assert_eq!(first.generation, "1");
    assert_eq!(first.cache_control, LATEST);
    assert!(
        first.etag.starts_with('"') && first.etag.len() == 34,
        "{}",
        first.etag
    );
    let compact = id.replace('-', "");
    assert!(first.tags.contains(":c:articles"), "{}", first.tags);
    assert!(first.tags.contains(&compact), "{}", first.tags);
    assert_eq!(titles(&first.body), ["First"]);

    let again = read(&list, None).await;
    assert_eq!(again.served, "hit");
    assert_eq!(
        (again.etag.as_str(), &again.body),
        (first.etag.as_str(), &first.body)
    );

    // F06: a new matching article is in the listing at once, from new keys.
    assert_eq!(publish_article(&owner, &id, "Second").await, 2);
    let after = read(&list, None).await;
    assert_eq!(
        (after.served.as_str(), after.generation.as_str()),
        ("miss", "2")
    );
    let mut seen = titles(&after.body);
    seen.sort();
    assert_eq!(seen, ["First", "Second"]);
    assert_ne!(after.etag, first.etag);

    // Generation 1 is still cached under its own key, and is shared longer: it can't change.
    let pinned = read(&format!("{list}?generation=1"), None).await;
    assert_eq!(pinned.served, "hit");
    assert_eq!(pinned.cache_control, PINNED);
    assert_eq!(titles(&pinned.body), ["First"]);

    // Revalidation: unchanged answers are 304, with the headers and no body.
    let unchanged = read(&list, Some(&after.etag)).await;
    assert_eq!(unchanged.status, StatusCode::NOT_MODIFIED);
    assert_eq!(unchanged.etag, after.etag);
    assert_eq!(unchanged.generation, "2");
    assert_eq!(unchanged.body, Value::Null);
    let changed = read(&list, Some(&first.etag)).await;
    assert_eq!(
        changed.status,
        StatusCode::OK,
        "generation 1's ETag is not generation 2's"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_hot_key_is_read_from_postgres_once_however_many_ask() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    publish_article(&owner, &id, "Hot").await;
    let listed = read(
        &format!("/content/v1/{id}/production/articles?limit=1"),
        None,
    )
    .await;
    let entry = listed.body["data"][0]["id"]
        .as_str()
        .expect("an entry")
        .to_owned();
    // The entry itself hasn't been read yet: its key is cold.
    let path = format!("/content/v1/{id}/production/articles/{entry}");
    let mut tasks = tokio::task::JoinSet::new();
    for _ in 0..200 {
        let path = path.clone();
        tasks.spawn(async move { read(&path, None).await });
    }
    let mut served: BTreeMap<String, usize> = BTreeMap::new();
    while let Some(joined) = tasks.join_next().await {
        let answer = joined.expect("task");
        assert_eq!(answer.status, StatusCode::OK);
        assert_eq!(answer.body["data"]["title"], "Hot");
        *served.entry(answer.served).or_default() += 1;
    }
    assert_eq!(served.get("miss"), Some(&1), "{served:?}");
    assert_eq!(served.get("bypass"), None, "{served:?}");
    assert_eq!(served.values().sum::<usize>(), 200);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_same_collection_in_two_projects_never_shares_an_answer() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let (one, two) = (project(&owner).await, project(&owner).await);
    publish_article(&owner, &one, "In one").await;
    publish_article(&owner, &two, "In two").await;
    for _ in 0..2 {
        let a = read(&format!("/content/v1/{one}/production/articles"), None).await;
        let b = read(&format!("/content/v1/{two}/production/articles"), None).await;
        assert_eq!(titles(&a.body), ["In one"]);
        assert_eq!(titles(&b.body), ["In two"]);
        assert_ne!(a.etag, b.etag);
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn nothing_missing_or_made_up_is_cached() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    publish_article(&owner, &id, "Real").await;
    let missing = format!("/content/v1/{id}/production/articles/{}", uuid_like());
    let made_up = format!("/content/v1/{id}/production/no_such_collection");
    for _ in 0..2 {
        assert_eq!(read(&missing, None).await.status, StatusCode::NOT_FOUND);
        let empty = read(&made_up, None).await;
        assert_eq!(empty.status, StatusCode::OK);
        assert_eq!(
            empty.served, "bypass",
            "an empty page is served, not cached"
        );
        assert_eq!(titles(&empty.body), Vec::<String>::new());
    }
}

/// A random UUID-shaped id, without a dependency for one.
fn uuid_like() -> String {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_nanos());
    let hex = format!("{nanos:032x}");
    format!(
        "{}-{}-4{}-8{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[13..16],
        &hex[17..20],
        &hex[20..32]
    )
}

/// Stops Valkey, and starts it again on drop even when an assertion fails halfway.
struct ValkeyStopped;

impl ValkeyStopped {
    fn compose(action: &str) {
        let file = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../deploy/compose.yaml");
        let status = Command::new("docker")
            .args(["compose", "-f"])
            .arg(file)
            .args([action, "valkey"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .expect("docker compose runs");
        assert!(status.success(), "docker compose {action} valkey failed");
    }

    fn new() -> Self {
        Self::compose("stop");
        Self
    }
}

impl Drop for ValkeyStopped {
    fn drop(&mut self) {
        Self::compose("start");
    }
}

#[tokio::test]
#[ignore = "needs the local stack, and stops Valkey: scripts/stack.sh up"]
async fn answers_survive_valkey_going_away_and_caching_resumes_when_it_is_back() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let id = project(&owner).await;
    publish_article(&owner, &id, "Survivor").await;
    let list = format!("/content/v1/{id}/production/articles");
    assert_eq!(read(&list, None).await.served, "miss");

    {
        let _stopped = ValkeyStopped::new();
        for _ in 0..3 {
            let started = Instant::now();
            let answer = read(&list, None).await;
            assert_eq!(answer.status, StatusCode::OK);
            assert_eq!(answer.served, "bypass");
            assert_eq!(titles(&answer.body), ["Survivor"]);
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "{:?}",
                started.elapsed()
            );
        }
        // A release while the cache is down is served at once, from Postgres.
        publish_article(&owner, &id, "While down").await;
        let answer = read(&list, None).await;
        assert_eq!(
            (answer.served.as_str(), answer.generation.as_str()),
            ("bypass", "2")
        );
    }

    // Back: the server reconnects by itself, and caches again.
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let answer = read(&list, None).await;
        assert_eq!(answer.status, StatusCode::OK);
        if answer.served == "hit" {
            assert_eq!(answer.generation, "2");
            break;
        }
        assert!(
            Instant::now() < deadline,
            "caching never resumed: still {}",
            answer.served
        );
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}
