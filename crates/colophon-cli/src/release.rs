//! `colophon release list` and `colophon release show` (docs/modules/08-cli.md): the
//! environment's releases, and one release's receipt — what was published, to which
//! destinations each delivery went, and what the probes actually observed. Reads only:
//! publishing stays a Studio/API decision a person takes.

use reqwest::Method;
use serde_json::json;

use crate::{
    client::Failure,
    out::{Format, Out},
    scope::{self, Scope},
};

fn releases_path(project: &str, scope: &Scope<'_>) -> String {
    format!("{}/releases", scope::env_path(project, scope))
}

/// Runs `colophon release list`, newest first. The `colophon-generation` answer header is
/// the environment's current generation and is reported beside the page.
pub(crate) async fn list(scope: &Scope<'_>, out: Out) -> Result<(), Failure> {
    let (client, project) = scope::open(scope).await?;
    let reply = client
        .send(Method::GET, &releases_path(&project, scope), None, &[])
        .await?;
    let generation = reply
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    let rows = reply.body["data"].as_array().cloned().unwrap_or_default();
    match out.format {
        Format::Json => out.print_data(&json!({
            "generation": generation,
            "releases": rows,
        })),
        Format::Jsonl => {
            for row in &rows {
                out.print_item(row);
            }
        }
        Format::Text => {
            if rows.is_empty() {
                println!("Nothing has been published yet.");
            } else {
                for row in &rows {
                    println!(
                        "generation {}  {}  {}  {}",
                        row["generation"].as_i64().unwrap_or_default(),
                        row["published_at"].as_str().unwrap_or(""),
                        row["label"].as_str().unwrap_or(""),
                        row["title"].as_str().unwrap_or("")
                    );
                }
            }
        }
    }
    Ok(())
}

/// Runs `colophon release show GENERATION`: the release and its receipt — deliveries per
/// destination and the probes' observations.
pub(crate) async fn show(scope: &Scope<'_>, generation: u64, out: Out) -> Result<(), Failure> {
    let (client, project) = scope::open(scope).await?;
    let release = client
        .send(
            Method::GET,
            &format!("{}/{generation}", releases_path(&project, scope)),
            None,
            &[],
        )
        .await?
        .body["data"]
        .clone();
    if out.format.machine() {
        out.print_data(&json!({ "release": release }));
    } else {
        println!(
            "Release {} — {}",
            release["generation"].as_i64().unwrap_or_default(),
            release["label"].as_str().unwrap_or("?")
        );
        println!(
            "  published {} by {}",
            release["published_at"].as_str().unwrap_or("?"),
            release["published_by_name"].as_str().unwrap_or("?")
        );
        println!(
            "  changeset {} ({})",
            release["changeset_id"].as_str().unwrap_or("?"),
            release["title"].as_str().unwrap_or("")
        );
        println!(
            "  {} published, {} taken down; purge {}; seen at {} location(s)",
            release["entries"].as_i64().unwrap_or_default(),
            release["unpublished"].as_i64().unwrap_or_default(),
            release["purge"].as_str().unwrap_or("?"),
            release["locations"].as_i64().unwrap_or_default()
        );
        let deliveries = release["deliveries"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !deliveries.is_empty() {
            println!("Deliveries:");
            for delivery in &deliveries {
                let covered = delivery["covered_by"]
                    .as_i64()
                    .map(|by| format!(" (covered by generation {by})"))
                    .unwrap_or_default();
                println!(
                    "  {} ({})  {}{}  {} in {} attempt(s)",
                    delivery["name"].as_str().unwrap_or("?"),
                    delivery["kind"].as_str().unwrap_or("?"),
                    delivery["state"].as_str().unwrap_or("?"),
                    covered,
                    delivery["status"].as_str().unwrap_or(""),
                    delivery["attempts"].as_i64().unwrap_or_default()
                );
            }
        }
        let observations = release["observations"]
            .as_array()
            .cloned()
            .unwrap_or_default();
        if !observations.is_empty() {
            println!("Observations:");
            for observation in &observations {
                println!(
                    "  {}  generation {}  HTTP {}  {}",
                    observation["location"].as_str().unwrap_or("?"),
                    observation["observed_generation"]
                        .as_i64()
                        .unwrap_or_default(),
                    observation["http_status"].as_i64().unwrap_or_default(),
                    observation["observed_at"].as_str().unwrap_or("")
                );
            }
        }
    }
    Ok(())
}
