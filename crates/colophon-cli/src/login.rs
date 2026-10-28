//! `colophon login`, `logout` and `whoami` (docs/modules/08-cli.md; D-041).
//!
//! `login` asks the server for a device code, tells the person where to approve it (on stderr:
//! it's an instruction, not a result), polls until they do, and keeps the access token in the
//! credentials file. The result goes to stdout.

use std::time::{Duration, Instant};

use reqwest::Method;
use serde_json::{Value, json};

use crate::{
    Exit,
    client::{Client, Failure},
    credentials::{self, Profile},
};

fn print_json(value: &Value) {
    println!("{value}");
}

/// Runs `colophon login`.
pub(crate) async fn login(
    server: &str,
    profile: &str,
    name: &str,
    json: bool,
) -> Result<(), Failure> {
    let client = Client::new(server, None)?;
    client.check_server().await?;
    let started = client
        .send(
            Method::POST,
            "/auth/device",
            Some(&json!({ "client_name": name })),
            &[],
        )
        .await?
        .body;
    let data = &started["data"];
    let (Some(secret), Some(code), Some(link)) = (
        data["device_secret"].as_str(),
        data["user_code"].as_str(),
        data["verification_uri"].as_str(),
    ) else {
        return Err(Failure::new(
            Exit::Internal,
            "the server's device sign-in answer is incomplete",
        ));
    };
    let interval = Duration::from_secs(data["interval"].as_u64().unwrap_or(2).clamp(1, 30));
    let deadline =
        Instant::now() + Duration::from_secs(data["expires_in"].as_u64().unwrap_or(600).min(3_600));
    eprintln!("To sign in, open this link in a browser where you're signed in to Colophon:");
    eprintln!("  {link}");
    eprintln!("and check that it shows the code {code}.");
    eprintln!("Waiting for approval…");

    loop {
        if Instant::now() >= deadline {
            return Err(Failure::new(
                Exit::Denied,
                "the code expired before it was approved; run `colophon login` again",
            ));
        }
        tokio::time::sleep(interval).await;
        let reply = match client
            .send(
                Method::POST,
                "/auth/device/token",
                Some(&json!({ "device_secret": secret })),
                &[],
            )
            .await
        {
            Ok(reply) => reply,
            // A lost poll changes nothing: ask again.
            Err(failure) if failure.exit == Exit::Unavailable => continue,
            Err(failure) => return Err(failure),
        };
        let data = &reply.body["data"];
        if data["status"] == "pending" {
            continue;
        }
        let (Some(token), Some(expires_at)) =
            (data["access_token"].as_str(), data["expires_at"].as_str())
        else {
            return Err(Failure::new(
                Exit::Internal,
                "the server's token answer is incomplete",
            ));
        };
        credentials::save(
            profile,
            Profile {
                server: server.to_owned(),
                token: token.to_owned(),
                expires_at: expires_at.to_owned(),
            },
        )
        .await?;
        let who = Client::new(server, Some(token.to_owned()))?
            .send(Method::GET, "/me", None, &[])
            .await?
            .body;
        let person = who["data"]["display_name"]
            .as_str()
            .or_else(|| who["data"]["id"].as_str())
            .unwrap_or("you");
        if json {
            print_json(&json!({
                "version": 1, "command": "login", "status": "signed_in", "server": server,
                "profile": profile, "person": person, "expires_at": expires_at
            }));
        } else {
            println!("Signed in to {server} as {person} (profile {profile}, until {expires_at}).");
        }
        return Ok(());
    }
}

/// Runs `colophon logout`: revokes the token on the server, then forgets it here. It skips the
/// version check on purpose: signing out must work even against a server this colophon can't
/// otherwise use, and `/auth/logout` predates version reports.
pub(crate) async fn logout(profile: &str) -> Result<(), Failure> {
    let saved = credentials::profile(profile).await?;
    let client = Client::new(&saved.server, Some(saved.token))?;
    match client.send(Method::POST, "/auth/logout", None, &[]).await {
        // Already revoked or expired: nothing left to revoke.
        Ok(_) => {}
        Err(failure) if failure.exit == Exit::Denied => {}
        Err(failure) => return Err(failure),
    }
    credentials::forget(profile).await?;
    println!("Signed out of {} (profile {profile}).", saved.server);
    Ok(())
}

/// Runs `colophon whoami`.
pub(crate) async fn whoami(profile: &str, json: bool) -> Result<(), Failure> {
    let saved = credentials::profile(profile).await?;
    let client = Client::new(&saved.server, Some(saved.token))?;
    client.check_server().await?;
    let me = client.send(Method::GET, "/me", None, &[]).await?.body;
    if json {
        print_json(
            &json!({ "version": 1, "command": "whoami", "server": saved.server, "me": me["data"] }),
        );
    } else {
        let name = me["data"]["display_name"].as_str().unwrap_or("(no name)");
        let administrator = if me["data"]["administrator"] == true {
            ", administrator"
        } else {
            ""
        };
        println!("{name} on {}{administrator}", saved.server);
        println!(
            "  subject {} at {}",
            me["data"]["subject"].as_str().unwrap_or("?"),
            me["data"]["issuer"].as_str().unwrap_or("?")
        );
        for project in me["data"]["projects"].as_array().into_iter().flatten() {
            println!(
                "  {}  {}  ({})",
                project["slug"].as_str().unwrap_or("?"),
                project["name"].as_str().unwrap_or("?"),
                project["role"].as_str().unwrap_or("?")
            );
        }
    }
    Ok(())
}
