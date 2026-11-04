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
    let projects = client
        .send(Method::GET, "/api/v1/projects", None, &[])
        .await?
        .body;
    let id = projects["data"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|project| project["id"] == scope.project || project["slug"] == scope.project)
        .and_then(|project| project["id"].as_str())
        .ok_or_else(|| {
            Failure::new(
                Exit::Denied,
                format!("no project {:?} that you're a member of", scope.project),
            )
        })?
        .to_owned();
    Ok((client, id))
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
