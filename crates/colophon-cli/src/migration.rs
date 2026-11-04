//! `colophon migration …` (docs/modules/08-cli.md; D-100): the plans a `content type` change
//! too unsafe to apply gets — computed as a dry run, approved and applied against exactly the
//! hash a person looked at, and finished or cancelled. These are human commands: the CLI's
//! caller is a signed-in member, and the API still checks `schema.change` on every one.
//!
//! `approve`, `apply` and `resolve` bind the plan hash with `If-Match` (`--if-match`, the
//! `hash` a list or show prints); every write reuses one `Idempotency-Key` on retry (C4).

use std::path::Path;

use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Exit,
    client::Failure,
    content::Place,
    scope::{self, Scope},
};

fn plans_path(project: &str, scope: &Scope<'_>) -> String {
    format!("{}/migrations", scope::env_path(project, scope))
}

/// A plan id, checked before it goes in a path.
fn plan_id(text: &str) -> Result<String, Failure> {
    scope::id_arg(text, "the migration")
}

/// Runs `colophon migration list`.
pub(crate) async fn list(
    scope: &Scope<'_>,
    collection: Option<&str>,
    state: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let (client, project) = scope::open(scope).await?;
    let mut path = plans_path(&project, scope);
    let mut separator = '?';
    for (name, value) in [("collection", collection), ("state", state)] {
        if let Some(value) = value {
            let value = scope::query_value(value, name)?;
            path.push(separator);
            path.push_str(name);
            path.push('=');
            path.push_str(value);
            separator = '&';
        }
    }
    let reply = client.send(Method::GET, &path, None, &[]).await?.body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({ "version": 1, "command": "migration list", "migrations": rows })
        );
    } else if rows.is_empty() {
        println!("No migration plans.");
    } else {
        for row in &rows {
            println!(
                "{}  {}  {} v{}\u{2192}{}  {}/{} items ({} blocked)  {}",
                row["id"].as_str().unwrap_or("?"),
                row["state"].as_str().unwrap_or("?"),
                row["collection"].as_str().unwrap_or("?"),
                row["base_version"].as_i64().unwrap_or_default(),
                row["target_version"]
                    .as_i64()
                    .map_or_else(|| "?".to_owned(), |version| format!("v{version}")),
                row["items"]["done"].as_i64().unwrap_or_default(),
                row["items"]["expected"].as_i64().unwrap_or_default(),
                row["items"]["blocked"].as_i64().unwrap_or_default(),
                row["created_at"].as_str().unwrap_or("")
            );
        }
    }
    Ok(())
}

/// Runs `colophon migration show MIGRATION`.
pub(crate) async fn show(scope: &Scope<'_>, migration: &str, json: bool) -> Result<(), Failure> {
    let id = plan_id(migration)?;
    let (client, project) = scope::open(scope).await?;
    let reply = client
        .send(
            Method::GET,
            &format!("{}/{id}", plans_path(&project, scope)),
            None,
            &[],
        )
        .await?;
    let plan = &reply.body["data"];
    if json {
        println!(
            "{}",
            json!({ "version": 1, "command": "migration show", "migration": plan })
        );
    } else {
        println!(
            "Plan {} — {}",
            plan["id"].as_str().unwrap_or("?"),
            plan["state"].as_str().unwrap_or("?")
        );
        println!(
            "  {} v{} \u{2192} {}",
            plan["collection"].as_str().unwrap_or("?"),
            plan["base_version"].as_i64().unwrap_or_default(),
            plan["target_version"]
                .as_i64()
                .map_or_else(|| "not yet".to_owned(), |version| format!("v{version}"))
        );
        println!(
            "  hash {} (the --if-match for approve, apply and resolve)",
            plan["hash"].as_str().unwrap_or("?")
        );
        println!(
            "  items {}/{} done, {} blocked; republish needed: {}",
            plan["items"]["done"].as_i64().unwrap_or_default(),
            plan["items"]["expected"].as_i64().unwrap_or_default(),
            plan["items"]["blocked"].as_i64().unwrap_or_default(),
            plan["republish_required"].as_bool().unwrap_or_default()
        );
        for (label, key) in [
            ("created", "created_at"),
            ("approved", "approved_at"),
            ("started", "started_at"),
            ("finished", "finished_at"),
        ] {
            if let Some(at) = plan[key].as_str() {
                println!("  {label} {at}");
            }
        }
        if let Some(error) = plan["error"].as_str() {
            println!("  error: {error}");
        }
    }
    Ok(())
}

/// Runs `colophon migration items MIGRATION`.
pub(crate) async fn items(
    scope: &Scope<'_>,
    migration: &str,
    outcome: Option<&str>,
    cursor: Option<&str>,
    limit: u8,
    json: bool,
) -> Result<(), Failure> {
    let id = plan_id(migration)?;
    let cursor = cursor.map(scope::cursor).transpose()?;
    let (client, project) = scope::open(scope).await?;
    let mut path = format!("{}/{id}/items?limit={limit}", plans_path(&project, scope));
    if let Some(outcome) = outcome {
        path.push_str("&outcome=");
        path.push_str(outcome);
    }
    if let Some(cursor) = cursor {
        path.push_str("&after=");
        path.push_str(cursor);
    }
    let reply = client.send(Method::GET, &path, None, &[]).await?.body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "migration items",
                "migration": id,
                "items": rows,
                "next_cursor": reply["next_cursor"],
            })
        );
    } else {
        if rows.is_empty() {
            println!("No items.");
        }
        for row in &rows {
            println!(
                "{}  {}  {}",
                row["entry_id"].as_str().unwrap_or("?"),
                row["outcome"].as_str().unwrap_or("?"),
                row["title"].as_str().unwrap_or(""),
            );
        }
        if let Some(cursor) = reply["next_cursor"].as_str() {
            eprintln!("More items: pass --cursor {cursor}");
        }
    }
    Ok(())
}

/// Runs `colophon migration plan`: computes the dry run for `definition` on the collection,
/// stores it, and prints its id and hash. The plan touches nothing; `approve` does.
pub(crate) async fn plan(
    place: &Place<'_>,
    definition: &Value,
    operations: Option<&Path>,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let operations = match operations {
        Some(file) => Some(file_json(file).await?),
        None => None,
    };
    let (client, project) = scope::open(&place.scope).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let mut body = json!({ "collection": place.collection, "definition": definition });
    if let Some(operations) = operations {
        body["operations"] = operations;
    }
    let reply = scope::send_write(
        &client,
        Method::POST,
        &plans_path(&project, &place.scope),
        Some(&body),
        &[("idempotency-key", &key)],
    )
    .await?;
    finish_write(&reply.headers, &reply.body, "plan", &key, json, |plan| {
        format!(
            "Planned migration {} on {}: {}/{} items, {} needing input. Hash: {}",
            plan["id"].as_str().unwrap_or("?"),
            plan["collection"].as_str().unwrap_or("?"),
            plan["items"]["done"].as_i64().unwrap_or_default(),
            plan["items"]["expected"].as_i64().unwrap_or_default(),
            plan["items"]["blocked"].as_i64().unwrap_or_default(),
            plan["hash"].as_str().unwrap_or("?"),
        )
    });
    Ok(())
}

/// The `approve`/`apply`/`cancel` lifecycle: one POST with an `Idempotency-Key`, and for
/// `approve` and `apply` the plan hash as `If-Match`.
pub(crate) async fn act(
    scope: &Scope<'_>,
    migration: &str,
    action: &'static str,
    if_match: Option<&str>,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let id = plan_id(migration)?;
    let (client, project) = scope::open(scope).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let tag = if_match.map(scope::etag_arg).transpose()?;
    let quoted = tag.map(|tag| format!("\"{tag}\""));
    let mut headers = vec![("idempotency-key", key.as_str())];
    if let Some(quoted) = &quoted {
        headers.push(("if-match", quoted.as_str()));
    }
    let reply = scope::send_write(
        &client,
        Method::POST,
        &format!("{}/{id}/{action}", plans_path(&project, scope)),
        None,
        &headers,
    )
    .await?;
    finish_write(&reply.headers, &reply.body, action, &key, json, |plan| {
        format!(
            "Plan {} is now {}.",
            plan["id"].as_str().unwrap_or(&id),
            plan["state"].as_str().unwrap_or("?")
        )
    });
    Ok(())
}

/// Runs `colophon migration resolve MIGRATION ENTRY`: stores `resolution` as the document an
/// item that needs input gets.
pub(crate) async fn resolve(
    scope: &Scope<'_>,
    migration: &str,
    entry: &str,
    resolution: &Value,
    if_match: &str,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let id = plan_id(migration)?;
    let entry = scope::id_arg(entry, "the entry")?;
    let tag = scope::etag_arg(if_match)?;
    let quoted = format!("\"{tag}\"");
    let (client, project) = scope::open(scope).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let reply = scope::send_write(
        &client,
        Method::PUT,
        &format!("{}/{id}/items/{entry}", plans_path(&project, scope)),
        Some(&json!({ "resolution": resolution })),
        &[("idempotency-key", &key), ("if-match", &quoted)],
    )
    .await?;
    finish_write(&reply.headers, &reply.body, "resolve", &key, json, |data| {
        format!(
            "Item {} of plan {} is resolved.",
            data["item"]["entry_id"].as_str().unwrap_or("?"),
            id
        )
    });
    Ok(())
}

/// A file's JSON, or a usage error.
async fn file_json(file: &Path) -> Result<Value, Failure> {
    let text = tokio::fs::read_to_string(file).await.map_err(|error| {
        Failure::new(
            Exit::Usage,
            format!("could not read {}: {error}", file.display()),
        )
    })?;
    serde_json::from_str::<Value>(&text).map_err(|error| {
        Failure::new(
            Exit::Usage,
            format!("{} is not JSON: {error}", file.display()),
        )
    })
}

/// Prints a write's answer: the replay flag, the key and the plan or item `data` holds.
fn finish_write(
    headers: &reqwest::header::HeaderMap,
    body: &Value,
    command: &str,
    key: &str,
    json: bool,
    say: impl FnOnce(&Value) -> String,
) {
    let replayed = headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let data = &body["data"];
    if json {
        println!(
            "{}",
            json!({
                "version": 1,
                "command": format!("migration {command}"),
                "status": if replayed { "replayed" } else { "ok" },
                "idempotency_key": key,
                "migration": data,
            })
        );
    } else {
        println!(
            "{}{}",
            if replayed { "Already applied: " } else { "" },
            say(data)
        );
    }
}
