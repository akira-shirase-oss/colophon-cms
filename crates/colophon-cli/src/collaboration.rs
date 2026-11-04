//! `colophon comment …` and `colophon task …` (docs/modules/08-cli.md; D-102): comments bound
//! to the revision they were written on, and the environment's editorial tasks. Writes carry
//! an `Idempotency-Key` and retry with the same one (C4); `task update` binds the task's `ETag`
//! with `If-Match` so it changes exactly what was looked at.

use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::{
    Exit,
    client::Failure,
    content::Place,
    scope::{self, Scope},
};

fn comments_path(project: &str, place: &Place<'_>, entry: &str) -> String {
    format!(
        "{}/{entry}/comments",
        scope::entries_path(project, &place.scope, place.collection)
    )
}

fn tasks_path(project: &str, scope: &Scope<'_>) -> String {
    format!("{}/tasks", scope::env_path(project, scope))
}

/// Runs `colophon comment list COLLECTION ENTRY`.
pub(crate) async fn list_comments(
    place: &Place<'_>,
    entry: &str,
    revision: Option<&str>,
    state: Option<&str>,
    limit: u8,
    cursor: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let entry = scope::id_arg(entry, "the entry")?;
    let revision = revision
        .map(|id| scope::id_arg(id, "--revision"))
        .transpose()?;
    let cursor = cursor.map(scope::cursor).transpose()?;
    let (client, project) = scope::open(&place.scope).await?;
    let mut path = format!("{}?limit={limit}", comments_path(&project, place, &entry));
    if let Some(revision) = &revision {
        path.push_str("&revision=");
        path.push_str(revision);
    }
    if let Some(state) = state {
        path.push_str("&state=");
        path.push_str(state);
    }
    if let Some(cursor) = cursor {
        path.push_str("&cursor=");
        path.push_str(cursor);
    }
    let reply = client.send(Method::GET, &path, None, &[]).await?.body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "comment list",
                "collection": place.collection,
                "entry": entry,
                "comments": rows,
                "next_cursor": reply["next_cursor"],
            })
        );
    } else {
        if rows.is_empty() {
            println!("No comments.");
        }
        for row in &rows {
            let body: String = row["body"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(70)
                .collect();
            println!(
                "{}  {}  {}  {}",
                row["id"].as_str().unwrap_or("?"),
                if row["resolved_at"].is_null() {
                    "open"
                } else {
                    "resolved"
                },
                row["created_at"].as_str().unwrap_or(""),
                body.lines().next().unwrap_or("")
            );
        }
        if let Some(cursor) = reply["next_cursor"].as_str() {
            eprintln!("More comments: pass --cursor {cursor}");
        }
    }
    Ok(())
}

/// Runs `colophon comment create COLLECTION ENTRY`.
pub(crate) async fn create_comment(
    place: &Place<'_>,
    entry: &str,
    revision: &str,
    field: Option<&str>,
    body_text: &str,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let entry = scope::id_arg(entry, "the entry")?;
    let revision = scope::id_arg(revision, "--revision")?;
    if body_text.trim().is_empty() {
        return Err(Failure::new(
            Exit::Usage,
            "a comment's --body can't be empty",
        ));
    }
    let (client, project) = scope::open(&place.scope).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let mut body = json!({ "revision": revision, "body": body_text });
    if let Some(field) = field {
        body["field"] = json!(field);
    }
    let reply = scope::send_write(
        &client,
        Method::POST,
        &comments_path(&project, place, &entry),
        Some(&body),
        &[("idempotency-key", &key)],
    )
    .await?;
    let replayed = reply
        .headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let comment = &reply.body["data"];
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "comment create",
                "status": if replayed { "replayed" } else { "created" },
                "idempotency_key": key,
                "comment": comment,
            })
        );
    } else {
        println!(
            "{} comment {} on {}/{}",
            if replayed {
                "Already created:"
            } else {
                "Created"
            },
            comment["id"].as_str().unwrap_or("?"),
            place.collection,
            entry
        );
    }
    Ok(())
}

/// Runs `colophon task list`.
#[expect(
    clippy::too_many_arguments,
    reason = "the command's flags, grouped where they can be: scope, filters, page, format"
)]
pub(crate) async fn list_tasks(
    scope: &Scope<'_>,
    entry: Option<&str>,
    collection: Option<&str>,
    assignee: Option<&str>,
    status: Option<&str>,
    limit: u8,
    cursor: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let entry = entry.map(|id| scope::id_arg(id, "--entry")).transpose()?;
    let collection = collection
        .map(|slug| scope::query_value(slug, "--collection"))
        .transpose()?;
    let assignee = match assignee {
        None => None,
        Some(special @ ("me" | "none")) => Some(special.to_owned()),
        Some(id) => Some(scope::id_arg(id, "--assignee")?),
    };
    let cursor = cursor.map(scope::cursor).transpose()?;
    let (client, project) = scope::open(scope).await?;
    let mut path = format!("{}?limit={limit}", tasks_path(&project, scope));
    for (name, value) in [
        ("entry", entry.as_deref()),
        ("collection", collection),
        ("assignee", assignee.as_deref()),
        ("status", status),
        ("cursor", cursor),
    ] {
        if let Some(value) = value {
            path.push('&');
            path.push_str(name);
            path.push('=');
            path.push_str(value);
        }
    }
    let reply = client.send(Method::GET, &path, None, &[]).await?.body;
    let rows = reply["data"].as_array().cloned().unwrap_or_default();
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "task list",
                "tasks": rows,
                "next_cursor": reply["next_cursor"],
            })
        );
    } else {
        if rows.is_empty() {
            println!("No tasks.");
        }
        for row in &rows {
            println!(
                "{}  {}  {}  {}",
                row["id"].as_str().unwrap_or("?"),
                row["status"].as_str().unwrap_or("?"),
                row["title"].as_str().unwrap_or(""),
                row["etag"].as_str().unwrap_or("")
            );
        }
        if let Some(cursor) = reply["next_cursor"].as_str() {
            eprintln!("More tasks: pass --cursor {cursor}");
        }
    }
    Ok(())
}

/// Runs `colophon task show TASK`.
pub(crate) async fn show_task(scope: &Scope<'_>, task: &str, json: bool) -> Result<(), Failure> {
    let id = scope::id_arg(task, "the task")?;
    let (client, project) = scope::open(scope).await?;
    let task = client
        .send(
            Method::GET,
            &format!("{}/{id}", tasks_path(&project, scope)),
            None,
            &[],
        )
        .await?
        .body["data"]
        .clone();
    if json {
        println!(
            "{}",
            json!({ "version": 1, "command": "task show", "task": task })
        );
    } else {
        println!(
            "Task {} — {}",
            task["id"].as_str().unwrap_or("?"),
            task["status"].as_str().unwrap_or("?")
        );
        println!("  {}", task["title"].as_str().unwrap_or(""));
        println!(
            "  entry {} ({})",
            task["entry_id"].as_str().unwrap_or("?"),
            task["collection"].as_str().unwrap_or("?")
        );
        println!(
            "  assignee {}  etag {} (the --if-match for `task update`)",
            task["assignee_id"].as_str().unwrap_or("none"),
            task["etag"].as_str().unwrap_or("?")
        );
    }
    Ok(())
}

/// Runs `colophon task create`.
#[expect(
    clippy::too_many_arguments,
    reason = "the command's flags, grouped where they can be: scope, entry, fields, key, format"
)]
pub(crate) async fn create_task(
    scope: &Scope<'_>,
    entry: &str,
    title: &str,
    revision: Option<&str>,
    changeset: Option<&str>,
    assignee: Option<&str>,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let entry = scope::id_arg(entry, "--entry")?;
    if title.trim().is_empty() {
        return Err(Failure::new(Exit::Usage, "a task's --title can't be empty"));
    }
    let revision = revision
        .map(|id| scope::id_arg(id, "--revision"))
        .transpose()?;
    let changeset = changeset
        .map(|id| scope::id_arg(id, "--changeset"))
        .transpose()?;
    let assignee = assignee
        .map(|id| scope::id_arg(id, "--assignee"))
        .transpose()?;
    let (client, project) = scope::open(scope).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let mut body = json!({ "entry": entry, "title": title });
    for (name, value) in [
        ("revision", revision),
        ("changeset", changeset),
        ("assignee", assignee),
    ] {
        if let Some(value) = value {
            body[name] = json!(value);
        }
    }
    let reply = scope::send_write(
        &client,
        Method::POST,
        &tasks_path(&project, scope),
        Some(&body),
        &[("idempotency-key", &key)],
    )
    .await?;
    let replayed = reply
        .headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let task = &reply.body["data"];
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "task create",
                "status": if replayed { "replayed" } else { "created" },
                "idempotency_key": key,
                "task": task,
            })
        );
    } else {
        println!(
            "{} task {} ({})",
            if replayed {
                "Already created:"
            } else {
                "Created"
            },
            task["id"].as_str().unwrap_or("?"),
            task["title"].as_str().unwrap_or("")
        );
    }
    Ok(())
}

/// Runs `colophon task update TASK`: one PATCH, bound to the task's `ETag`. `--unassign` writes
/// `assignee: null`; each absent flag leaves its column.
#[expect(
    clippy::too_many_arguments,
    reason = "the command's flags, grouped where they can be: scope, task, etag, changes, key, format"
)]
pub(crate) async fn update_task(
    scope: &Scope<'_>,
    task: &str,
    if_match: &str,
    title: Option<&str>,
    assignee: Option<&str>,
    unassign: bool,
    status: Option<&str>,
    key: Option<&str>,
    json: bool,
) -> Result<(), Failure> {
    let id = scope::id_arg(task, "the task")?;
    let tag = scope::etag_arg(if_match)?;
    let quoted = format!("\"{tag}\"");
    let mut body = Map::new();
    if let Some(title) = title {
        if title.trim().is_empty() {
            return Err(Failure::new(Exit::Usage, "a task's --title can't be empty"));
        }
        body.insert("title".to_owned(), json!(title));
    }
    if let Some(assignee) = assignee {
        body.insert(
            "assignee".to_owned(),
            json!(scope::id_arg(assignee, "--assignee")?),
        );
    } else if unassign {
        body.insert("assignee".to_owned(), Value::Null);
    }
    if let Some(status) = status {
        body.insert("status".to_owned(), json!(status));
    }
    if body.is_empty() {
        return Err(Failure::new(
            Exit::Usage,
            "nothing to change: give --title, --assignee/--unassign or --status",
        ));
    }
    let (client, project) = scope::open(scope).await?;
    let key = key.map_or_else(scope::new_key, str::to_owned);
    let reply = scope::send_write(
        &client,
        Method::PATCH,
        &format!("{}/{id}", tasks_path(&project, scope)),
        Some(&Value::Object(body)),
        &[("idempotency-key", &key), ("if-match", &quoted)],
    )
    .await?;
    let replayed = reply
        .headers
        .get("idempotent-replayed")
        .is_some_and(|value| value == "true");
    let task = &reply.body["data"];
    if json {
        println!(
            "{}",
            json!({
                "version": 1, "command": "task update",
                "status": if replayed { "replayed" } else { "updated" },
                "idempotency_key": key,
                "task": task,
            })
        );
    } else {
        println!(
            "{} task {} is now {}",
            if replayed {
                "Already updated:"
            } else {
                "Updated:"
            },
            task["id"].as_str().unwrap_or("?"),
            task["status"].as_str().unwrap_or("?")
        );
    }
    Ok(())
}
