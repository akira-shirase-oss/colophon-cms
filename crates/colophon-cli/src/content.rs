//! `colophon content …` (docs/modules/08-cli.md): the same API the Studio uses, so the CLI
//! creates exactly the same records.
//!
//! Writes send one `Idempotency-Key` per logical operation and reuse it when a request gets
//! no answer, so a retry can never create a second entry (`scope::send_write`). If every
//! attempt fails, the key is printed: running the command again with `--idempotency-key` is
//! just as safe.

use std::{io::Write as _, path::Path};

use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Exit,
    client::{Client, Failure},
    out::{Format, Out},
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
    out: Out,
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
    if out.format.machine() {
        out.print_data(&json!({
            "status": if replayed { "replayed" } else { "created" },
            "idempotency_key": key,
            "entry": { "id": data["id"], "collection": data["collection"], "revision": data["revision"]["id"], "etag": etag, "content": data["content"] }
        }));
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

/// The `?limit=&cursor=` of one page of a collection's entries.
fn list_path(project: &str, place: &Place<'_>, limit: u8, cursor: Option<&str>) -> String {
    let mut path = format!("{}?limit={limit}", entries(project, place));
    if let Some(cursor) = cursor {
        path.push_str("&cursor=");
        path.push_str(cursor);
    }
    path
}

/// Runs `colophon content list`: one page of rows, or — on `--jsonl` — every page, one
/// object per line (`scope::stream_jsonl`).
pub(crate) async fn list(
    place: &Place<'_>,
    limit: u8,
    cursor: Option<&str>,
    out: Out,
) -> Result<(), Failure> {
    let cursor = cursor.map(scope::cursor).transpose()?.map(str::to_owned);
    let (client, project) = open(place).await?;
    if out.format == Format::Jsonl {
        return scope::stream_jsonl(
            &client,
            Method::GET,
            |page| list_path(&project, place, limit, page),
            None,
            cursor,
            &out,
        )
        .await;
    }
    let reply = client
        .send(
            Method::GET,
            &list_path(&project, place, limit, cursor.as_deref()),
            None,
            &[],
        )
        .await?
        .body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if out.format.machine() {
        out.print_data(&json!({
            "collection": place.collection,
            "entries": rows,
            "next_cursor": reply["next_cursor"],
        }));
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
        if let Some(cursor) = reply["next_cursor"].as_str() {
            eprintln!("More entries: pass --cursor {cursor}");
        }
    }
    Ok(())
}

/// Runs `colophon content query`: posts a query document and gets one page of entries back,
/// with the canonical query and its hash the server actually ran (`applied`), so scripts can
/// pin exactly the page they got. On `--jsonl` every page streams, one object per line; the
/// `applied` report is the `--json` answer's.
pub(crate) async fn query(
    place: &Place<'_>,
    document: &Value,
    cursor: Option<&str>,
    limit: Option<u8>,
    out: Out,
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
    let cursor = cursor.map(scope::cursor).transpose()?.map(str::to_owned);
    let (client, project) = open(place).await?;
    let base = format!("{}/query", entries(&project, place));
    let path = |page: Option<&str>| match page {
        Some(page) => format!("{base}?cursor={page}"),
        None => base.clone(),
    };
    if out.format == Format::Jsonl {
        return scope::stream_jsonl(&client, Method::POST, path, Some(&document), cursor, &out)
            .await;
    }
    let reply = client
        .send(Method::POST, &path(cursor.as_deref()), Some(&document), &[])
        .await?
        .body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if out.format.machine() {
        out.print_data(&json!({
            "collection": place.collection,
            "entries": rows,
            "next_cursor": reply["next_cursor"],
            "applied": reply["applied"],
        }));
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

/// The most items the API's import accepts in one file (`IMPORT_MAX_ITEMS`, D-086).
const IMPORT_MAX_ITEMS: usize = 512;

/// The largest body a Colophon server can take — `COLOPHON_MAX_BODY_BYTES` tops out at
/// 64 MiB — so the CLI's own bound on an import file or on one line of an export, which a
/// stored document can never legitimately exceed.
const MAX_FILE_BYTES: u64 = 64 * 1024 * 1024;

/// The most lines the API's export ever carries (its own `LIMIT`, D-086); a longer stream
/// is a misbehaving server — the bound on what `--json` holds at once.
const EXPORT_MAX_ITEMS: usize = 10_000;

/// The import's body: `--data` or `--file`'s text, checked the way the API will read it
/// (`check_import_body`), and bounded by the largest request body a server can take so a
/// file that could never be accepted is refused here, not after the upload.
pub(crate) async fn import_body(
    data: Option<&str>,
    file: Option<&Path>,
) -> Result<String, Failure> {
    let text = match (data, file) {
        (Some(data), None) => data.to_owned(),
        (None, Some(file)) => {
            if let Ok(metadata) = tokio::fs::metadata(file).await
                && metadata.len() > MAX_FILE_BYTES
            {
                return Err(Failure::new(
                    Exit::Usage,
                    format!(
                        "{} is {} bytes — bigger than the largest request body a Colophon \
                         server accepts (64 MiB)",
                        file.display(),
                        metadata.len()
                    ),
                ));
            }
            tokio::fs::read_to_string(file).await.map_err(|error| {
                Failure::new(
                    Exit::Usage,
                    format!("could not read {}: {error}", file.display()),
                )
            })?
        }
        _ => {
            return Err(Failure::new(
                Exit::Usage,
                "give the import with exactly one of --data or --file",
            ));
        }
    };
    check_import_body(&text)?;
    Ok(text)
}

/// `text` as the API's import reads it (entries.rs `import_items`): `[` begins a JSON
/// array; anything else is one JSON object a line — a `{"data":[…],"meta":{…}}` envelope
/// reads as one line, and which shape it is the API decides. Every element or line must
/// parse, an empty file is a usage error rather than a silent no-op, and past
/// `IMPORT_MAX_ITEMS` the API would refuse anyway.
fn check_import_body(text: &str) -> Result<(), Failure> {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return Err(Failure::new(
            Exit::Usage,
            "the file is empty — there is nothing to import",
        ));
    }
    let items = if trimmed.starts_with('[') {
        serde_json::from_str::<Vec<Value>>(trimmed)
            .map_err(|error| {
                Failure::new(
                    Exit::Usage,
                    format!("the file starts a JSON array but doesn't parse as one: {error}"),
                )
            })?
            .len()
    } else {
        let mut items = 0usize;
        for (index, line) in trimmed.lines().enumerate() {
            if line.trim().is_empty() {
                continue;
            }
            serde_json::from_str::<Value>(line)
                .map_err(|_| Failure::new(Exit::Usage, format!("line {} isn't JSON", index + 1)))?;
            items += 1;
        }
        items
    };
    if items == 0 {
        return Err(Failure::new(
            Exit::Usage,
            "the file holds no items to import",
        ));
    }
    if items > IMPORT_MAX_ITEMS {
        return Err(Failure::new(
            Exit::Usage,
            format!("an import is at most {IMPORT_MAX_ITEMS} items; this file has {items}"),
        ));
    }
    Ok(())
}

/// Runs `colophon content import`: posts the file to the collection's import, verbatim —
/// the API sniffs the shape (native JSONL, a JSON array, a Strapi envelope, Strapi or
/// Sanity documents) and lands it in one transaction or not at all (D-086, D-114). A dry
/// run plans every item and writes nothing, needing no key; an applied import sends one
/// `Idempotency-Key`, generated when omitted, so a retried answer can't import twice.
pub(crate) async fn import(
    place: &Place<'_>,
    body: String,
    dry_run: bool,
    source_type: Option<&str>,
    key: Option<&str>,
    out: Out,
) -> Result<(), Failure> {
    let (client, project) = open(place).await?;
    let mut path = format!(
        "{}/collections/{}/import",
        scope::env_path(&project, &place.scope),
        place.collection
    );
    let mut separator = '?';
    if dry_run {
        path.push_str("?dry_run=true");
        separator = '&';
    }
    if let Some(kind) = source_type {
        path.push(separator);
        path.push_str("type=");
        path.push_str(kind);
    }
    // A plan is not an operation: it records nothing, so there is no key to claim.
    let key = (!dry_run).then(|| key.map_or_else(scope::new_key, str::to_owned));
    let headers: Vec<(&str, &str)> = key
        .iter()
        .map(|key| ("idempotency-key", key.as_str()))
        .collect();
    // The content type names what the file's first byte suggests; the API sniffs the same.
    let media = if body.trim_start().starts_with('[') {
        "application/json"
    } else {
        "application/jsonlines"
    };
    let reply =
        scope::send_write_text(&client, Method::POST, &path, &body, media, &headers).await?;
    let replayed = reply
        .headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let data = &reply.body["data"];
    if out.format.machine() {
        let mut view = data.clone();
        view["collection"] = json!(place.collection);
        view["status"] = json!(if dry_run {
            "planned"
        } else if replayed {
            "replayed"
        } else {
            "imported"
        });
        if let Some(key) = &key {
            view["idempotency_key"] = json!(key);
        }
        out.print_data(&view);
    } else {
        let outcomes = data["outcomes"].as_array().cloned().unwrap_or_default();
        if dry_run {
            let mut parts = Vec::new();
            for (status, word) in [
                ("would_create", "would create"),
                ("would_update", "would update"),
                ("skipped", "skipped"),
                ("invalid", "invalid"),
            ] {
                let count = outcomes.iter().filter(|o| o["status"] == status).count();
                if count > 0 {
                    parts.push(format!("{count} {word}"));
                }
            }
            println!("Plan for {}: {}.", place.collection, parts.join(", "));
        } else {
            let imported = data["imported"].as_u64().unwrap_or(0);
            println!(
                "{} {} {} into {} ({} created, {} updated).",
                if replayed {
                    "Already imported"
                } else {
                    "Imported"
                },
                imported,
                if imported == 1 { "entry" } else { "entries" },
                place.collection,
                outcomes.iter().filter(|o| o["status"] == "created").count(),
                outcomes.iter().filter(|o| o["status"] == "updated").count(),
            );
        }
        // Every item that didn't write cleanly, named by the pointer the API gave it.
        for outcome in &outcomes {
            let item = outcome["item"].as_str().unwrap_or("?");
            match outcome["status"].as_str() {
                Some("skipped") => println!(
                    "  {item} skipped: {}",
                    outcome["reason"].as_str().unwrap_or("")
                ),
                Some("invalid") => println!(
                    "  {item} invalid: {} ({} at {})",
                    outcome["refusal"]["message"].as_str().unwrap_or(""),
                    outcome["refusal"]["code"].as_str().unwrap_or("?"),
                    outcome["refusal"]["pointer"].as_str().unwrap_or("?"),
                ),
                _ => {}
            }
        }
    }
    Ok(())
}

/// One export line's document as a value; a line that isn't JSON means the export the
/// server wrote was corrupt — an internal failure, in whichever format asked for it.
fn export_item(line: &[u8]) -> Result<Value, Failure> {
    serde_json::from_slice(line)
        .map_err(|_| Failure::new(Exit::Internal, "the export carried a line that isn't JSON"))
}

/// One export line, dispatched on the format: whitespace skipped (the file's closing
/// newline ends one), else text writes the document's own bytes back (a `> posts.jsonl`
/// redirect is a byte-faithful export), `--jsonl` wraps it in the item envelope, and
/// `--json` collects it under the API's own bound.
fn export_line(
    line: &[u8],
    out: Out,
    items: &mut Vec<Value>,
    count: &mut usize,
) -> Result<(), Failure> {
    if line.iter().all(u8::is_ascii_whitespace) {
        return Ok(());
    }
    *count += 1;
    let item = export_item(line)?;
    match out.format {
        Format::Text => {
            let mut stdout = std::io::stdout().lock();
            stdout
                .write_all(line)
                .and_then(|()| stdout.write_all(b"\n"))
                .map_err(|error| {
                    Failure::new(
                        Exit::Internal,
                        format!("could not write to stdout: {error}"),
                    )
                })?;
        }
        Format::Jsonl => out.print_item(&item),
        Format::Json => {
            if items.len() >= EXPORT_MAX_ITEMS {
                return Err(Failure::new(
                    Exit::Internal,
                    format!(
                        "the export carried more than {EXPORT_MAX_ITEMS} entries — \
                         the API's own bound"
                    ),
                ));
            }
            items.push(item);
        }
    }
    Ok(())
}

/// Runs `colophon content export`: the collection's drafts stream to stdout — the raw
/// file on text, one envelope per entry on `--jsonl`, the whole collection as one object
/// on `--json`. Items print as their line arrives; the stream never holds more than a
/// line, and only `--json`'s collection grows — bounded by the API's own cap (D-123).
pub(crate) async fn export(place: &Place<'_>, out: Out) -> Result<(), Failure> {
    let (client, project) = open(place).await?;
    let path = format!(
        "{}/collections/{}/export",
        scope::env_path(&project, &place.scope),
        place.collection
    );
    let mut response = client.send_stream(Method::GET, &path, &[]).await?;
    let mut pending: Vec<u8> = Vec::new();
    let mut items: Vec<Value> = Vec::new();
    let mut count = 0usize;
    while let Some(chunk) = response.chunk().await.map_err(|error| {
        Failure::new(
            Exit::Unavailable,
            format!("the export stopped arriving: {error}"),
        )
    })? {
        pending.extend_from_slice(&chunk);
        if pending.len() as u64 > MAX_FILE_BYTES {
            return Err(Failure::new(
                Exit::Internal,
                "one line of the export is bigger than a document can be",
            ));
        }
        while let Some(end) = pending.iter().position(|byte| *byte == b'\n') {
            let mut line: Vec<u8> = pending.drain(..=end).collect();
            line.pop();
            export_line(&line, out, &mut items, &mut count)?;
        }
    }
    // The server ends every line, an empty file included: a byte left over is a line cut
    // mid-way, not an item.
    if pending.iter().any(|byte| !u8::is_ascii_whitespace(byte)) {
        return Err(Failure::new(
            Exit::Internal,
            "the export's last line never ended",
        ));
    }
    if out.format == Format::Json {
        out.print_data(&json!({
            "collection": place.collection,
            "entries": items,
        }));
    } else if out.format == Format::Text {
        // The file itself was stdout; the count is the diagnostic (an empty collection
        // would otherwise look like nothing happened).
        eprintln!(
            "Exported {count} {} from {}.",
            if count == 1 { "entry" } else { "entries" },
            place.collection
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_import_body_is_jsonl_an_array_or_an_envelope_line() {
        // JSONL: one object a line, blank lines forgiven.
        check_import_body("{\"title\": \"a\"}\n\n{\"title\": \"b\"}\n").expect("JSONL");
        // A JSON array.
        check_import_body("[{\"title\": \"a\"}, {\"title\": \"b\"}]").expect("array");
        // A Strapi envelope reads as one line; its shape is the API's call.
        check_import_body("{\"data\": [{\"title\": \"a\"}], \"meta\": {}}")
            .expect("an envelope is one line");
    }

    #[test]
    fn an_import_body_says_which_line_failed() {
        let failure = check_import_body("{\"title\": \"a\"}\nnot json\n").expect_err("refused");
        assert_eq!(failure.exit, Exit::Usage);
        assert!(failure.message.contains("line 2"), "{}", failure.message);
        // A `[` body that doesn't close the array.
        let failure =
            check_import_body("[{\"title\": \"a\"},\n{\"title\": \"b\"}").expect_err("refused");
        assert_eq!(failure.exit, Exit::Usage);
        assert!(failure.message.contains("array"), "{}", failure.message);
    }

    #[test]
    fn an_empty_import_is_a_usage_error_not_a_no_op() {
        for text in ["", "  \n\t\n", "[]"] {
            let failure = check_import_body(text).expect_err("refused");
            assert_eq!(failure.exit, Exit::Usage, "{text:?}");
        }
    }

    #[test]
    fn an_import_body_is_bounded_like_the_api_s() {
        let body = (0..=IMPORT_MAX_ITEMS)
            .map(|index| format!("{{\"title\": \"{index}\"}}"))
            .collect::<Vec<_>>()
            .join("\n");
        let failure = check_import_body(&body).expect_err("refused");
        assert_eq!(failure.exit, Exit::Usage);
        assert!(failure.message.contains("512"), "{}", failure.message);
        let body = (0..IMPORT_MAX_ITEMS)
            .map(|index| format!("{{\"title\": \"{index}\"}}"))
            .collect::<Vec<_>>()
            .join("\n");
        check_import_body(&body).expect("512 is the bound, not a refusal");
    }

    #[test]
    fn an_export_line_must_be_json() {
        export_item(b"{\"title\": \"a\"}").expect("a document");
        assert!(
            export_item(b"\"a string\"").is_ok(),
            "any JSON value counts as a line"
        );
        assert_eq!(
            export_item(b"{cut off").expect_err("refused").exit,
            Exit::Internal
        );
    }
}
