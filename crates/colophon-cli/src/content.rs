//! `colophon content …` (docs/modules/08-cli.md): the same API the Studio uses, so the CLI
//! creates exactly the same records.
//!
//! Writes send one `Idempotency-Key` per logical operation and reuse it when a request gets
//! no answer, so a retry can never create a second entry (`scope::send_write`). If every
//! attempt fails, the key is printed: running the command again with `--idempotency-key` is
//! just as safe.

use std::path::Path;

use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Exit,
    client::{Client, Failure},
    scope::{self, Scope},
};

/// Where an entry lives.
pub(crate) struct Place<'a> {
    pub(crate) scope: Scope<'a>,
    pub(crate) collection: &'a str,
}

/// The signed-in client and the project's id (`--project` may be an id or a slug).
async fn open(place: &Place<'_>) -> Result<(Client, String), Failure> {
    scope::open(&place.scope).await
}

fn entries(project: &str, place: &Place<'_>) -> String {
    scope::entries_path(project, &place.scope, place.collection)
}

/// The JSON a flag names: `--data` as JSON, or `--file`.
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
                "give the document with exactly one of --data or --file",
            ));
        }
    };
    serde_json::from_str(&text)
        .map_err(|error| Failure::new(Exit::Usage, format!("the document is not JSON: {error}")))
}

/// Runs `colophon content create`.
pub(crate) async fn create(
    place: &Place<'_>,
    content: &Value,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let (client, project) = open(place).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let reply = scope::send_write(
        &client,
        Method::POST,
        &entries(&project, place),
        Some(content),
        &[("idempotency-key", &key)],
    )
    .await?;
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

/// Runs `colophon content query`: posts a query document and gets one page of entries back,
/// with the canonical query and its hash the server actually ran (`applied`), so scripts can
/// pin exactly the page they got.
pub(crate) async fn query(
    place: &Place<'_>,
    document: &Value,
    cursor: Option<&str>,
    limit: Option<u8>,
    json: bool,
) -> Result<(), Failure> {
    let mut document = document.clone();
    if let Some(limit) = limit {
        let object = document
            .as_object_mut()
            .ok_or_else(|| Failure::new(Exit::Usage, "the query document must be a JSON object"))?;
        if object.contains_key("limit") {
            return Err(Failure::new(
                Exit::Usage,
                "the document already sets a limit; keep it there or drop --limit",
            ));
        }
        object.insert("limit".to_owned(), json!(limit));
    }
    let cursor = cursor.map(scope::cursor).transpose()?;
    let (client, project) = open(place).await?;
    let mut path = format!("{}/query", entries(&project, place));
    if let Some(cursor) = cursor {
        path.push_str("?cursor=");
        path.push_str(cursor);
    }
    let reply = client
        .send(Method::POST, &path, Some(&document), &[])
        .await?
        .body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "content query",
                "collection": place.collection,
                "entries": rows,
                "next_cursor": reply["next_cursor"],
                "applied": reply["applied"],
            })
        );
    } else {
        for row in &rows {
            println!(
                "{}  {}  {}",
                row["id"].as_str().unwrap_or("?"),
                row["updated_at"].as_str().unwrap_or(""),
                row["revision"]["title"].as_str().unwrap_or("")
            );
        }
        if rows.is_empty() {
            println!("No entries matched.");
        }
        if let Some(cursor) = reply["next_cursor"].as_str() {
            eprintln!("More entries: pass --cursor {cursor}");
        }
    }
    Ok(())
}
