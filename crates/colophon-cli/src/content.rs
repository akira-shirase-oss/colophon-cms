//! `colophon content create` and `colophon content list` (docs/modules/08-cli.md): the same API
//! the Studio uses, so the CLI creates exactly the same records.
//!
//! `create` sends one `Idempotency-Key` per logical operation and reuses it when a request gets
//! no answer, so a retry can never create a second entry. If every attempt fails, the key is
//! printed: running the command again with `--idempotency-key` is just as safe.

use std::{path::Path, time::Duration};

use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Exit,
    client::{Client, Failure},
    credentials,
};

/// Attempts at a create before giving up (the first plus retries), and the waits between.
const BACKOFF: [Duration; 3] = [
    Duration::from_secs(1),
    Duration::from_secs(2),
    Duration::from_secs(4),
];

/// Where an entry lives.
pub(crate) struct Place<'a> {
    pub(crate) profile: &'a str,
    pub(crate) project: &'a str,
    pub(crate) environment: &'a str,
    pub(crate) collection: &'a str,
}

/// The signed-in client and the project's id (`--project` may be an id or a slug).
async fn open(place: &Place<'_>) -> Result<(Client, String), Failure> {
    let saved = credentials::profile(place.profile).await?;
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
        .find(|project| project["id"] == place.project || project["slug"] == place.project)
        .and_then(|project| project["id"].as_str())
        .ok_or_else(|| {
            Failure::new(
                Exit::Denied,
                format!("no project {:?} that you're a member of", place.project),
            )
        })?
        .to_owned();
    Ok((client, id))
}

fn entries(project: &str, place: &Place<'_>) -> String {
    format!(
        "/api/v1/projects/{project}/environments/{}/collections/{}/entries",
        place.environment, place.collection
    )
}

/// The content to create: `--data` as JSON, or `--file`.
pub(crate) async fn content(data: Option<&str>, file: Option<&Path>) -> Result<Value, Failure> {
    let text = match (data, file) {
        (Some(data), None) => data.to_owned(),
        (None, Some(file)) => tokio::fs::read_to_string(file).await.map_err(|error| {
            Failure::new(
                Exit::Usage,
                format!("could not read {}: {error}", file.display()),
            )
        })?,
        _ => {
            return Err(Failure::new(
                Exit::Usage,
                "give the content with exactly one of --data or --file",
            ));
        }
    };
    serde_json::from_str(&text)
        .map_err(|error| Failure::new(Exit::Usage, format!("the content is not JSON: {error}")))
}

/// Runs `colophon content create`.
pub(crate) async fn create(
    place: &Place<'_>,
    content: &Value,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let (client, project) = open(place).await?;
    let key = key.map_or_else(
        || format!("cli-{}", uuid::Uuid::now_v7().simple()),
        str::to_owned,
    );
    let path = entries(&project, place);
    let mut attempt = 0;
    let reply = loop {
        match client
            .send(
                Method::POST,
                &path,
                Some(content),
                &[("idempotency-key", &key)],
            )
            .await
        {
            Ok(reply) => break reply,
            Err(failure) if failure.exit == Exit::Unavailable && attempt < BACKOFF.len() => {
                eprintln!("{}; retrying with the same key", failure.message);
                tokio::time::sleep(BACKOFF[attempt]).await;
                attempt += 1;
            }
            Err(failure) if failure.exit == Exit::Unavailable => {
                return Err(Failure::new(
                    Exit::Unavailable,
                    format!(
                        "{}. Nothing was created twice; to try again safely, add --idempotency-key {key}",
                        failure.message
                    ),
                ));
            }
            Err(failure) => return Err(failure),
        }
    };
    let replayed = reply
        .headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let data = &reply.body["data"];
    let etag = reply
        .headers
        .get("etag")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "content create",
                "status": if replayed { "replayed" } else { "created" },
                "idempotency_key": key,
                "entry": { "id": data["id"], "collection": data["collection"], "revision": data["revision"]["id"], "etag": etag, "content": data["content"] }
            })
        );
    } else {
        println!(
            "{} {} {} ({})",
            if replayed {
                "Already created:"
            } else {
                "Created"
            },
            place.collection,
            data["id"].as_str().unwrap_or("?"),
            data["revision"]["title"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

/// Runs `colophon content list`.
pub(crate) async fn list(place: &Place<'_>, limit: u8, json: bool) -> Result<(), Failure> {
    let (client, project) = open(place).await?;
    let reply = client
        .send(
            Method::GET,
            &format!("{}?limit={limit}", entries(&project, place)),
            None,
            &[],
        )
        .await?
        .body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if json {
        let entries: Vec<Value> = rows
            .iter()
            .map(|row| json!({ "id": row["id"], "title": row["revision"]["title"], "revision": row["revision"]["id"], "updated_at": row["updated_at"] }))
            .collect();
        println!(
            "{}",
            json!({ "version": 1, "command": "content list", "collection": place.collection, "entries": entries, "next_cursor": reply["next_cursor"] })
        );
    } else if rows.is_empty() {
        println!("No entries in {}.", place.collection);
    } else {
        for row in &rows {
            println!(
                "{}  {}  {}",
                row["id"].as_str().unwrap_or("?"),
                row["updated_at"].as_str().unwrap_or(""),
                row["revision"]["title"].as_str().unwrap_or("")
            );
        }
    }
    Ok(())
}
