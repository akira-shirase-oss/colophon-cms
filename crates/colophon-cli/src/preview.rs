//! `colophon preview` (D-171, docs/modules/08-cli.md): opens a private preview of an entry's
//! saved draft — or of exactly what a publish request will publish — on the project's own
//! site, and prints the one-time address. The address works once, within a minute; the
//! preview it opens lasts the minutes asked for. Issuing is a person's act: an API token is
//! refused by the server (`api_token_not_allowed`).

use reqwest::Method;
use serde_json::json;

use crate::{
    client::Failure,
    out::Out,
    scope::{self, Scope},
};

/// What to preview.
pub(crate) enum Target<'a> {
    /// An entry, with its translations, following their saved drafts.
    Entry(&'a str),
    /// A publish request, bound to its hash.
    Changeset(&'a str),
}

/// Runs `colophon preview`.
pub(crate) async fn open(
    scope: &Scope<'_>,
    target: Target<'_>,
    minutes: u32,
    out: Out,
) -> Result<(), Failure> {
    let (client, project) = scope::open(scope).await?;
    let ttl_seconds = minutes * 60;
    let body = match target {
        Target::Entry(entry) => json!({ "entry": entry, "ttl_seconds": ttl_seconds }),
        Target::Changeset(changeset) => {
            json!({ "changeset": changeset, "ttl_seconds": ttl_seconds })
        }
    };
    let issued = client
        .send(
            Method::POST,
            &format!("{}/previews", scope::env_path(&project, scope)),
            Some(&body),
            &[],
        )
        .await?
        .body["data"]
        .clone();
    if out.format.machine() {
        out.print_data(&json!({ "preview": issued }));
    } else {
        println!("{}", issued["url"].as_str().unwrap_or("?"));
        println!(
            "Open it within a minute: the address works once. The preview ends at {}.",
            issued["expires_at"].as_str().unwrap_or("?")
        );
    }
    Ok(())
}
