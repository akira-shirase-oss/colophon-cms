//! Where most commands work: a project and one of its environments. The signed-in profile
//! finds the project (by id or slug) and checks the server is one this colophon fits; the
//! path helpers build the routes every scoped command shares; and `send_write` holds the
//! one retry rule (C4): a logical operation retries with its idempotency key, never a new
//! one.

use std::time::Duration;

use reqwest::Method;
use serde_json::Value;

use crate::{
    Exit,
    client::{Client, Failure, Reply},
    credentials,
    out::Out,
};

/// Attempts at a write before giving up (the first plus retries), and the waits between.
const BACKOFF: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// The scope a command works in: a signed-in profile, a project and an environment.
pub(crate) struct Scope<'a> {
    pub(crate) profile: &'a str,
    pub(crate) project: &'a str,
    pub(crate) environment: &'a str,
}

/// The signed-in client and the project's id (`scope.project` may be an id or a slug).
pub(crate) async fn open(scope: &Scope<'_>) -> Result<(Client, String), Failure> {
    let saved = credentials::profile(scope.profile).await?;
    let client = Client::new(&saved.server, Some(saved.token))?;
    client.check_server().await?;
    let id = resolve(
        |path| {
            let client = &client;
            async move { client.send(Method::GET, &path, None, &[]).await.map(|reply| reply.body) }
        },
        scope.project,
    )
    .await?;
    Ok((client, id))
}

/// The id of the project `project` names (an id or a URL name) among the caller's projects,
/// reading the API through `fetch` (a GET of a path, answering its JSON body).
///
/// An id is read directly (`GET /api/v1/projects/{id}`, which answers only a member). A URL name
/// is found through the bounded project search, a page of at most 100 at a time, the first exact
/// match by name as before. Neither depends on how many projects the caller is in: the unpaged
/// list stops at 1,000, so a member of more couldn't reach the rest (D-194).
pub(crate) async fn resolve<F, Fut>(fetch: F, project: &str) -> Result<String, Failure>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<Value, Failure>>,
{
    if let Ok(id) = uuid::Uuid::try_parse(project) {
        return match fetch(format!("/api/v1/projects/{id}")).await {
            Ok(body) => body["data"]["id"]
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| Failure::new(Exit::Internal, "the project's answer has no id")),
            Err(failure) if is_not_found(&failure) => Err(not_a_member(project)),
            Err(failure) => Err(failure),
        };
    }
    // A URL name is lowercase letters, digits and dashes (so it goes in the query as it is); anything
    // else names no project.
    let shaped = (1..=MAX_SLUG).contains(&project.len())
        && project
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-');
    if !shaped {
        return Err(not_a_member(project));
    }
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_SEARCH_PAGES {
        let mut path = format!("/api/v1/projects?q={project}&limit=100");
        if let Some(after) = &cursor {
            path.push_str("&cursor=");
            path.push_str(after);
        }
        let page = fetch(path).await?;
        if let Some(found) = page["data"]
            .as_array()
            .into_iter()
            .flatten()
            .find(|candidate| candidate["slug"] == project)
            .and_then(|candidate| candidate["id"].as_str())
        {
            return Ok(found.to_owned());
        }
        match page["next_cursor"].as_str() {
            Some(next) if is_cursor(next) => cursor = Some(next.to_owned()),
            _ => break,
        }
    }
    Err(not_a_member(project))
}

/// The longest URL name the search can carry (the API's search is at most 100 characters).
const MAX_SLUG: usize = 100;
/// Search pages read for one URL name before giving up: 1,000 pages of 100, far past any
/// membership count, so a server that never ends its cursor can't hold a command forever.
const MAX_SEARCH_PAGES: usize = 1_000;

/// A cursor the API sent, before it goes back in a URL (base64url, as [`cursor`] checks).
fn is_cursor(text: &str) -> bool {
    (1..=16_384).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_')
}

/// Whether a failure is the API's "not found", which for a project means "not a member" too.
fn is_not_found(failure: &Failure) -> bool {
    failure
        .refusal
        .as_ref()
        .is_some_and(|refusal| refusal["code"] == "not_found")
}

fn not_a_member(project: &str) -> Failure {
    Failure::new(
        Exit::Denied,
        format!("no project {project:?} that you're a member of"),
    )
}

/// The `/api/v1/projects/{project}/environments/{environment}` the scoped routes are under.
pub(crate) fn env_path(project: &str, scope: &Scope<'_>) -> String {
    format!(
        "/api/v1/projects/{project}/environments/{}",
        scope.environment
    )
}

/// The `…/collections/{collection}/entries` of a scope.
pub(crate) fn entries_path(project: &str, scope: &Scope<'_>, collection: &str) -> String {
    format!(
        "{}/collections/{collection}/entries",
        env_path(project, scope)
    )
}

/// A fresh idempotency key for one logical operation.
pub(crate) fn new_key() -> String {
    format!("cli-{}", uuid::Uuid::now_v7().simple())
}

/// A positional id, such as an entry's or a plan's, checked before it goes in a path.
pub(crate) fn id_arg(text: &str, what: &str) -> Result<String, Failure> {
    uuid::Uuid::try_parse(text)
        .map(|id| id.to_string())
        .map_err(|_| Failure::new(Exit::Usage, format!("{what} must be an id (a UUID)")))
}

/// A page cursor, exactly as `next_cursor` printed it — checked so it lands in the URL
/// whole (the API's cursors are base64url: letters, digits, `-` and `_`).
pub(crate) fn cursor(text: &str) -> Result<&str, Failure> {
    let shaped = (1..=16_384).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if shaped {
        Ok(text)
    } else {
        Err(Failure::new(
            Exit::Usage,
            "the cursor is next_cursor from the previous page, exactly as printed",
        ))
    }
}

/// A `?name=value` value that can go in a URL whole — slugs, ids and the list filters are
/// all `[A-Za-z0-9_-]`; anything else is refused before it can break the URL.
pub(crate) fn query_value<'a>(text: &'a str, what: &str) -> Result<&'a str, Failure> {
    let shaped = (1..=256).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_');
    if shaped {
        Ok(text)
    } else {
        Err(Failure::new(
            Exit::Usage,
            format!("{what} may only be letters, digits, '-' and '_'"),
        ))
    }
}

/// `query_value` widened to every RFC 3986 unreserved character — a Sanity `_type` such as
/// `blog.article` may hold `.` and `~` too. Anything else is refused rather than encoded, so
/// what `--type` names is exactly what the URL carries.
pub(crate) fn query_unreserved<'a>(text: &'a str, what: &str) -> Result<&'a str, Failure> {
    let shaped = (1..=256).contains(&text.len())
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~'));
    if shaped {
        Ok(text)
    } else {
        Err(Failure::new(
            Exit::Usage,
            format!("{what} may only be letters, digits, '-', '_', '.' and '~'"),
        ))
    }
}

/// An `ETag` for `If-Match`, as `show` and `list` print it (`sha256:<64 hex>`); surrounding
/// quotes are forgiven.
pub(crate) fn etag_arg(text: &str) -> Result<String, Failure> {
    let tag = text.trim().trim_matches('"');
    let shaped = tag.strip_prefix("sha256:").is_some_and(|hex| {
        hex.len() == 64
            && hex
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    });
    if shaped {
        Ok(tag.to_owned())
    } else {
        Err(Failure::new(
            Exit::Usage,
            "--if-match must be an ETag as `show` prints it, such as sha256:…",
        ))
    }
}

/// The most pages a `--jsonl` stream follows: a bound on a cursor that never ends (C5).
const MAX_STREAM_PAGES: u32 = 1_000;

/// Follows `next_cursor` to the list's end, printing each row as one `--jsonl` line as its
/// page arrives — the stream never holds more than a page's rows (D-119). `path` maps the
/// page's cursor (`None` on the first page) to the request's path; `body` rides along for
/// a `POST` such as `content query`. A page whose cursor isn't shaped like one, or a list
/// that outlives the bound, is an internal failure: the stream ends with the error
/// envelope on stdout and `error: …` on stderr like any other failure.
pub(crate) async fn stream_jsonl(
    client: &Client,
    method: Method,
    path: impl Fn(Option<&str>) -> String,
    body: Option<&Value>,
    first: Option<String>,
    out: &Out,
) -> Result<(), Failure> {
    let mut after = first;
    for _ in 0..MAX_STREAM_PAGES {
        let reply = client
            .send(method.clone(), &path(after.as_deref()), body, &[])
            .await?;
        let rows = reply.body["data"].as_array().cloned().unwrap_or_default();
        for row in &rows {
            out.print_item(row);
        }
        match reply.body["next_cursor"].as_str() {
            Some(next) => {
                after = Some(
                    cursor(next)
                        .map_err(|_| {
                            Failure::new(
                                Exit::Internal,
                                "the server sent a next_cursor that isn't one",
                            )
                        })?
                        .to_owned(),
                );
            }
            None => return Ok(()),
        }
    }
    Err(Failure::new(
        Exit::Internal,
        format!("the list didn't end after {MAX_STREAM_PAGES} pages"),
    ))
}

/// Sends a request that carries an `idempotency-key` header, retrying a lost answer with
/// the very same key so the write can't happen twice (C4). When every attempt fails, the
/// error names the key a retry can safely reuse.
pub(crate) async fn send_write(
    client: &Client,
    method: Method,
    path: &str,
    body: Option<&Value>,
    headers: &[(&str, &str)],
) -> Result<Reply, Failure> {
    write_with_retries(headers, || client.send(method.clone(), path, body, headers)).await
}

/// `send_write` for a body that isn't one JSON value — an import's file, sent as it is,
/// still under the operation's one key so a retry can't import twice.
pub(crate) async fn send_write_text(
    client: &Client,
    method: Method,
    path: &str,
    body: &str,
    content_type: &'static str,
    headers: &[(&str, &str)],
) -> Result<Reply, Failure> {
    write_with_retries(headers, || {
        client.send_text(method.clone(), path, body.to_owned(), content_type, headers)
    })
    .await
}

/// The retry loop every write shares: a lost answer is retried with `headers`' very same
/// `idempotency-key`, and the last failure names the key a retry can safely reuse.
async fn write_with_retries<F, Fut>(headers: &[(&str, &str)], mut send: F) -> Result<Reply, Failure>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<Reply, Failure>>,
{
    let key = headers
        .iter()
        .find(|(name, _)| *name == "idempotency-key")
        .map_or("?", |(_, value)| *value);
    let mut attempt = 0;
    loop {
        match send().await {
            Ok(reply) => return Ok(reply),
            Err(failure) if failure.exit == Exit::Unavailable && attempt < BACKOFF.len() => {
                eprintln!("{}; retrying with the same key", failure.message);
                tokio::time::sleep(BACKOFF[attempt]).await;
                attempt += 1;
            }
            Err(failure) if failure.exit == Exit::Unavailable => {
                return Err(Failure::new(
                    Exit::Unavailable,
                    format!(
                        "{}. To try again safely, run it again with --idempotency-key {key}",
                        failure.message
                    ),
                ));
            }
            Err(failure) => return Err(failure),
        }
    }
}

#[cfg(test)]
mod resolve_tests {
    use std::collections::HashMap;

    use serde_json::{Value, json};

    use super::resolve;
    use crate::client::Failure;

    /// A server whose caller is a member of `count` projects named alike ("Site 0001" …), as the
    /// API answers: the unpaged list is the first 1,000 by name, then id; `?q=` pages by name, at
    /// most `limit` (≤ 100) a page, with a cursor; `/projects/{id}` answers a member's project.
    struct Server {
        projects: Vec<(String, String, String)>, // (id, slug, name), sorted by name then id
    }

    impl Server {
        fn with(count: usize) -> Self {
            let mut projects: Vec<_> = (1..=count)
                .map(|n| {
                    (
                        format!("01a1ee00-0000-7000-8000-{n:012}"),
                        format!("site-{n:04}"),
                        format!("Site {n:04}"),
                    )
                })
                .collect();
            projects.sort_by(|a, b| (&a.2, &a.0).cmp(&(&b.2, &b.0)));
            Self { projects }
        }

        fn view((id, slug, name): &(String, String, String)) -> Value {
            json!({"id": id, "slug": slug, "name": name})
        }

        fn get(&self, path: &str) -> Result<Value, Failure> {
            let (route, query) = path.split_once('?').unwrap_or((path, ""));
            let params: HashMap<&str, &str> = query.split('&').filter_map(|pair| pair.split_once('=')).collect();
            if let Some(id) = route.strip_prefix("/api/v1/projects/") {
                return self
                    .projects
                    .iter()
                    .find(|project| project.0 == id)
                    .map(|project| json!({"data": Self::view(project)}))
                    .ok_or_else(|| Failure {
                        exit: crate::Exit::Denied,
                        message: "no such resource [HTTP 404]".into(),
                        refusal: Some(json!({"code": "not_found"})),
                    });
            }
            assert_eq!(route, "/api/v1/projects", "unexpected request {path}");
            if params.is_empty() {
                let data: Vec<_> = self.projects.iter().take(1000).map(Self::view).collect();
                return Ok(json!({"data": data, "next_cursor": null}));
            }
            let limit: usize = params.get("limit").and_then(|value| value.parse().ok()).unwrap_or(20);
            assert!((1..=100).contains(&limit), "limit {limit} is out of the API's range");
            let search = params.get("q").copied().unwrap_or("");
            let after = params.get("cursor").copied();
            let matching: Vec<_> = self
                .projects
                .iter()
                .filter(|project| project.1.contains(search) || project.2.to_lowercase().contains(search))
                .skip_while(|project| after.is_some_and(|cursor| project.0 != cursor))
                .skip(usize::from(after.is_some()))
                .collect();
            let page: Vec<_> = matching.iter().take(limit).map(|project| Self::view(project)).collect();
            let next = (matching.len() > limit).then(|| matching[limit - 1].0.clone());
            Ok(json!({"data": page, "next_cursor": next}))
        }
    }

    async fn resolved(server: &Server, project: &str) -> Result<String, String> {
        resolve(|path| async move { server.get(&path) }, project)
            .await
            .map_err(|failure| failure.message)
    }

    /// A member of more than 1,000 projects reaches every one of them, by id and by URL name;
    /// the 1,500th by name is past the unpaged list's 1,000.
    #[tokio::test]
    async fn a_member_of_more_than_a_thousand_projects_reaches_the_last_one() {
        let server = Server::with(1500);
        let last = server.projects.last().cloned().expect("invariant: 1,500 projects");
        assert_eq!(resolved(&server, &last.0).await, Ok(last.0.clone()), "by id");
        assert_eq!(resolved(&server, &last.1).await, Ok(last.0.clone()), "by URL name");
        let first = server.projects.first().cloned().expect("invariant: 1,500 projects");
        assert_eq!(resolved(&server, &first.1).await, Ok(first.0.clone()), "the first, by URL name");
    }

    /// A project the caller isn't in, or a name that can't be a URL name, is refused the same way.
    #[tokio::test]
    async fn a_project_the_caller_isnt_in_is_refused() {
        let server = Server::with(3);
        for project in ["01a1ee00-0000-7000-8000-999999999999", "nope", "Not A Slug!"] {
            let refused = resolved(&server, project).await.expect_err(project);
            assert!(refused.contains("that you're a member of"), "{project}: {refused}");
        }
    }
}
