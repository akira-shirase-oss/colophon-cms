//! SSRF through destinations (docs/security/attack-surface.md, gap D): a destination can't point into the
//! server's own network, by name, literal or IPv6 embedding. The local configuration allows private
//! destinations for the demo sites, so this server starts without that switch, as production runs.

use reqwest::{Method, StatusCode};
use serde_json::json;

use super::{refused, send};
use crate::common::{Server, project, sign_in};

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_destination_never_points_inside_the_network() {
    let _server = Server::start_with(&[("COLOPHON_ALLOW_PRIVATE_DESTINATIONS", "0")]).await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let path = format!("/api/v1/projects/{project}/environments/production/destinations");
    for url in [
        "http://hooks.example.com/build",
        "https://127.0.0.1/hook",
        "https://10.0.0.1/hook",
        "https://169.254.169.254/latest/meta-data/",
        "https://[::1]/hook",
        "https://[::ffff:127.0.0.1]/hook",
        "https://[::127.0.0.1]/hook",
        "https://[2002:7f00:1::]/hook",
        "https://[2002:a9fe:a9fe::]/hook",
        "https://[2001:0:4136:e378:8000:63bf:f5ff:fffe]/hook",
        "https://[fec0::1]/hook",
        "https://[fd00::1]/hook",
        "https://[64:ff9b::a9fe:a9fe]/hook",
        "https://user:secret@hooks.example.com/build",
        "ftp://hooks.example.com/build",
        "file:///etc/passwd",
        "gopher://127.0.0.1:6379/_FLUSHALL",
    ] {
        let reply = send(
            &owner,
            Method::POST,
            &path,
            Some(&json!({"name": "Hook", "url": url})),
        )
        .await;
        refused(&reply, url);
    }
    let listed = send(&owner, Method::GET, &path, None).await;
    assert_eq!(listed.status, StatusCode::OK, "{}", listed.body);
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(0),
        "{}",
        listed.body
    );
}
