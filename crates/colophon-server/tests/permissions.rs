//! Roles owners define, end to end (D-058, step R.2): the real binary, two real people. An owner
//! defines a role, the role decides what its holder may do from their next request, a change
//! names the version it edited, a role in use can't be deleted, the owner role can't be changed,
//! permissions that nothing enforces yet are refused, and nobody hands out more than they hold.
//! Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Reply, Server, Session, article, call, entries, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

fn roles(project: &str) -> String {
    format!("/api/v1/projects/{project}/roles")
}

async fn send(
    session: &Session,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    call(session, method, path, body, extra)
        .await
        .expect("request")
}

/// The project's owner's and its one other member's identity ids (owners are listed first).
async fn ids(owner: &Session, project: &str) -> (String, String) {
    let listed = send(
        owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/members"),
        None,
        &[],
    )
    .await;
    let id = |index: usize| {
        listed.body["data"][index]["identity_id"]
            .as_str()
            .expect("an id")
            .to_owned()
    };
    (id(0), id(1))
}

async fn write_article(session: &Session, project: &str) -> StatusCode {
    send(
        session,
        Method::POST,
        &entries(project),
        Some(&article("By a reader?", "Body")),
        &[("idempotency-key", &key())],
    )
    .await
    .status
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: define, hold, change, refuse, delete"
)]
async fn a_role_the_owner_defines_decides_what_its_holder_may_do() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;

    // A reader of articles, and nothing else.
    let made = send(
        &owner,
        Method::POST,
        &roles(&project),
        Some(&json!({
            "key": "reader",
            "name": "Reader",
            "description": "Reads articles.",
            "permissions": [{"action": "content.read", "subject": "articles"}]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.etag().as_deref(), Some("\"v1\""));
    assert_eq!(made.body["data"]["fixed"], false);
    join(&owner, &guest, &project, "reader").await;

    let listed = send(&guest, Method::GET, &roles(&project), None, &[]).await;
    let keys: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("roles")
        .iter()
        .filter_map(|role| role["key"].as_str())
        .collect();
    assert_eq!(
        keys,
        ["owner", "developer", "editor", "publisher", "reader"]
    );
    assert_eq!(listed.body["data"][4]["members"], 1);
    assert_eq!(listed.body["data"][0]["fixed"], true);

    let read = send(&guest, Method::GET, &entries(&project), None, &[]).await;
    assert_eq!(read.status, StatusCode::OK, "{}", read.body);
    assert_eq!(write_article(&guest, &project).await, StatusCode::FORBIDDEN);

    // The owner lets readers write articles; the guest's next request may.
    let reader = format!("{}/reader", roles(&project));
    let writer = json!({
        "name": "Article writer",
        "permissions": [
            {"action": "content.read", "subject": "articles"},
            {"action": "content.create", "subject": "articles"},
            {"action": "content.update", "subject": "articles"}
        ]
    });
    let changed = send(
        &owner,
        Method::PUT,
        &reader,
        Some(&writer),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(changed.status, StatusCode::OK, "{}", changed.body);
    assert_eq!(changed.etag().as_deref(), Some("\"v2\""));
    assert_eq!(write_article(&guest, &project).await, StatusCode::CREATED);

    // An edit of an older version is refused, and says which version is current.
    let stale = send(
        &owner,
        Method::PUT,
        &reader,
        Some(&writer),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT, "{}", stale.body);
    assert_eq!(stale.body["error"]["details"]["reason"], "stale_role");
    assert_eq!(stale.etag().as_deref(), Some("\"v2\""));
    let unconditional = send(&owner, Method::PUT, &reader, Some(&writer), &[]).await;
    assert_eq!(unconditional.status, StatusCode::PRECONDITION_REQUIRED);

    // Held, it can't be deleted; the owner role can't be changed at all.
    let held = send(
        &owner,
        Method::DELETE,
        &reader,
        None,
        &[("if-match", "\"v2\"")],
    )
    .await;
    assert_eq!(held.status, StatusCode::CONFLICT, "{}", held.body);
    assert_eq!(held.body["error"]["details"]["reason"], "role_in_use");
    let owner_role = format!("{}/owner", roles(&project));
    let fixed = send(
        &owner,
        Method::PUT,
        &owner_role,
        Some(&writer),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(fixed.status, StatusCode::CONFLICT, "{}", fixed.body);

    // Fields and own only where they mean something.
    for (permission, reason) in [
        (
            json!({"action": "content.everything", "subject": "*"}),
            "unknown_action",
        ),
        (
            json!({"action": "members.manage", "subject": "articles"}),
            "project_wide_action",
        ),
        (
            json!({"action": "content.publish", "subject": "articles", "fields": ["title"]}),
            "fields_not_applicable",
        ),
        (
            json!({"action": "content.create", "subject": "*", "condition": "own"}),
            "condition_not_applicable",
        ),
    ] {
        let refused = send(
            &owner,
            Method::POST,
            &roles(&project),
            Some(&json!({"key": "odd", "name": "Odd", "permissions": [permission]})),
            &[],
        )
        .await;
        assert_eq!(refused.status, StatusCode::UNPROCESSABLE_ENTITY);
        assert_eq!(
            refused.body["error"]["details"]["reason"],
            reason,
            "{refused:?}",
            refused = refused.body
        );
    }

    // Once nobody holds it, it goes.
    let (_, guest_id) = ids(&owner, &project).await;
    let member = format!("/api/v1/projects/{project}/members/{guest_id}");
    let moved = send(
        &owner,
        Method::PUT,
        &member,
        Some(&json!({"role": "editor"})),
        &[],
    )
    .await;
    assert_eq!(moved.status, StatusCode::NO_CONTENT, "{}", moved.body);
    let deleted = send(
        &owner,
        Method::DELETE,
        &reader,
        None,
        &[("if-match", "\"v2\"")],
    )
    .await;
    assert_eq!(deleted.status, StatusCode::NO_CONTENT, "{}", deleted.body);
    let gone = send(&owner, Method::GET, &reader, None, &[]).await;
    assert_eq!(gone.status, StatusCode::NOT_FOUND);
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
async fn nobody_hands_out_more_than_they_hold() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let lead = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &roles(&project),
        Some(&json!({
            "key": "team-lead",
            "name": "Team lead",
            "permissions": [
                {"action": "members.manage", "subject": "*"},
                {"action": "content.read", "subject": "*"},
                {"action": "content.create", "subject": "*"},
                {"action": "content.update", "subject": "*"}
            ]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &lead, &project, "team-lead").await;

    let invitations = format!("/api/v1/projects/{project}/invitations");
    for role in ["owner", "developer", "publisher"] {
        let refused = send(
            &lead,
            Method::POST,
            &invitations,
            Some(&json!({"role": role, "label": "Someone"})),
            &[],
        )
        .await;
        assert_eq!(refused.status, StatusCode::FORBIDDEN, "inviting as {role}");
    }
    let allowed = send(
        &lead,
        Method::POST,
        &invitations,
        Some(&json!({"role": "editor", "label": "Someone"})),
        &[],
    )
    .await;
    assert_eq!(allowed.status, StatusCode::CREATED, "{}", allowed.body);
    let unknown = send(
        &lead,
        Method::POST,
        &invitations,
        Some(&json!({"role": "nobody", "label": "Someone"})),
        &[],
    )
    .await;
    assert_eq!(unknown.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(unknown.body["error"]["details"]["reason"], "unknown_role");

    let (owner_id, lead_id) = ids(&owner, &project).await;
    let promote = send(
        &lead,
        Method::PUT,
        &format!("/api/v1/projects/{project}/members/{lead_id}"),
        Some(&json!({"role": "owner"})),
        &[],
    )
    .await;
    assert_eq!(promote.status, StatusCode::FORBIDDEN, "{}", promote.body);
    let remove_owner = send(
        &lead,
        Method::DELETE,
        &format!("/api/v1/projects/{project}/members/{owner_id}"),
        None,
        &[],
    )
    .await;
    assert_eq!(remove_owner.status, StatusCode::FORBIDDEN);

    // Managing members isn't managing roles.
    let role = send(
        &lead,
        Method::POST,
        &roles(&project),
        Some(&json!({"key": "mine", "name": "Mine", "permissions": []})),
        &[],
    )
    .await;
    assert_eq!(role.status, StatusCode::FORBIDDEN, "{}", role.body);
}

/// The article's draft as `session` sees it.
async fn read_article(session: &Session, project: &str, id: &str) -> Reply {
    send(
        session,
        Method::GET,
        &format!("{}/{id}", entries(project)),
        None,
        &[],
    )
    .await
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: an author's own articles and everyone else's"
)]
async fn an_author_reads_and_edits_titles_of_any_article_and_all_of_their_own() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let author = sign_in("dev-editor").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &roles(&project),
        Some(&json!({
            "key": "author",
            "name": "Author",
            "permissions": [
                {"action": "content.read", "subject": "articles", "fields": ["title"]},
                {"action": "content.read", "subject": "articles", "condition": "own"},
                {"action": "content.create", "subject": "articles"},
                {"action": "content.update", "subject": "articles", "fields": ["title"]},
                {"action": "content.update", "subject": "articles", "condition": "own"}
            ]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(
        made.body["data"]["permissions"].as_array().map(Vec::len),
        Some(5)
    );
    join(&owner, &author, &project, "author").await;

    // The owner's article: the author sees its title, not its body, and may change the title.
    let theirs = send(
        &owner,
        Method::POST,
        &entries(&project),
        Some(&article("Owner's note", "Private body")),
        &[("idempotency-key", &key())],
    )
    .await;
    let theirs_id = theirs.body["data"]["id"]
        .as_str()
        .expect("an id")
        .to_owned();
    let seen = read_article(&author, &project, &theirs_id).await;
    assert_eq!(seen.status, StatusCode::OK, "{}", seen.body);
    assert_eq!(
        seen.body["data"]["content"],
        json!({"title": "Owner's note"})
    );
    assert_eq!(seen.body["data"]["hidden_fields"], json!(["body"]));
    let revision = seen.etag().expect("an etag");
    let retitled = send(
        &author,
        Method::PUT,
        &format!("{}/{theirs_id}", entries(&project)),
        Some(&json!({"title": "Owner's note, retitled"})),
        &[("if-match", &revision), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(retitled.status, StatusCode::OK, "{}", retitled.body);
    // The body the author never saw is exactly as it was.
    let full = read_article(&owner, &project, &theirs_id).await;
    assert_eq!(
        full.body["data"]["content"]["title"],
        "Owner's note, retitled"
    );
    assert_eq!(
        full.body["data"]["content"]["body"],
        theirs.body["data"]["content"]["body"]
    );
    // A body sent for it anyway is refused, not ignored.
    let revision = retitled.etag().expect("an etag");
    let sneaky = send(
        &author,
        Method::PUT,
        &format!("{}/{theirs_id}", entries(&project)),
        Some(&article("Owner's note, retitled", "Rewritten")),
        &[("if-match", &revision), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(
        sneaky.status,
        StatusCode::UNPROCESSABLE_ENTITY,
        "{}",
        sneaky.body
    );
    assert_eq!(
        sneaky.body["error"]["details"]["reason"],
        "field_not_permitted"
    );
    assert_eq!(sneaky.body["error"]["details"]["pointer"], "/body");

    // Their own article: every field.
    let mine = send(
        &author,
        Method::POST,
        &entries(&project),
        Some(&article("Author's note", "Draft")),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(mine.status, StatusCode::CREATED, "{}", mine.body);
    let mine_id = mine.body["data"]["id"].as_str().expect("an id").to_owned();
    assert!(mine.body["data"].get("hidden_fields").is_none());
    let revision = mine.etag().expect("an etag");
    let rewritten = send(
        &author,
        Method::PUT,
        &format!("{}/{mine_id}", entries(&project)),
        Some(&article("Author's note", "Finished")),
        &[("if-match", &revision), ("idempotency-key", &key())],
    )
    .await;
    assert_eq!(rewritten.status, StatusCode::OK, "{}", rewritten.body);

    // The list shows both, with titles.
    let listed = send(&author, Method::GET, &entries(&project), None, &[]).await;
    let titles: Vec<&str> = listed.body["data"]
        .as_array()
        .expect("entries")
        .iter()
        .filter_map(|entry| entry["revision"]["title"].as_str())
        .collect();
    assert_eq!(titles, ["Author's note", "Owner's note, retitled"]);

    // Without the titles permissions, someone else's article is out of reach: not found to read,
    // and not something to pin into a publish request.
    let role = format!("{}/author", roles(&project));
    let narrowed = send(
        &owner,
        Method::PUT,
        &role,
        Some(&json!({
            "name": "Author",
            "permissions": [
                {"action": "content.read", "subject": "articles", "condition": "own"},
                {"action": "content.create", "subject": "articles"},
                {"action": "content.update", "subject": "articles", "condition": "own"}
            ]
        })),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(narrowed.status, StatusCode::OK, "{}", narrowed.body);
    assert_eq!(
        read_article(&author, &project, &theirs_id).await.status,
        StatusCode::NOT_FOUND
    );
    let listed = send(&author, Method::GET, &entries(&project), None, &[]).await;
    assert_eq!(
        listed.body["data"].as_array().map(Vec::len),
        Some(1),
        "only their own"
    );
    let changesets = format!("/api/v1/projects/{project}/environments/production/changesets");
    let pin_theirs = send(
        &author,
        Method::POST,
        &changesets,
        Some(&json!({"title": "Both", "entries": [theirs_id, mine_id]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(
        pin_theirs.status,
        StatusCode::FORBIDDEN,
        "{}",
        pin_theirs.body
    );
    let pin_mine = send(
        &author,
        Method::POST,
        &changesets,
        Some(&json!({"title": "Mine", "entries": [mine_id]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(pin_mine.status, StatusCode::CREATED, "{}", pin_mine.body);
}

/// Titles matching `q` in the project's articles, as `session` sees them.
async fn search(session: &Session, project: &str, q: &str) -> Reply {
    send(
        session,
        Method::GET,
        &format!("{}?q={q}", entries(project)),
        None,
        &[],
    )
    .await
}

fn titles(reply: &Reply) -> Vec<&str> {
    reply.body["data"]
        .as_array()
        .expect("a list")
        .iter()
        .map(|entry| entry["revision"]["title"].as_str().expect("a title"))
        .collect()
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_title_search_never_answers_from_titles_the_role_cant_read() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let guest = sign_in("dev-editor").await;
    let project = project(&owner).await;
    // Bodies of anyone's articles, but titles only of one's own.
    let made = send(
        &owner,
        Method::POST,
        &roles(&project),
        Some(&json!({
            "key": "body-reader",
            "name": "Body reader",
            "permissions": [
                {"action": "content.read", "subject": "articles", "fields": ["body"]},
                {"action": "content.read", "subject": "articles", "condition": "own"},
                {"action": "content.create", "subject": "articles"}
            ]
        })),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    join(&owner, &guest, &project, "body-reader").await;
    for (session, title) in [(&owner, "Secret launch plan"), (&guest, "My launch notes")] {
        let created = send(
            session,
            Method::POST,
            &entries(&project),
            Some(&article(title, "Body")),
            &[("idempotency-key", &key())],
        )
        .await;
        assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    }

    // The owner's title is hidden from the guest, so a search can't confirm a guess at it.
    let guessed = search(&guest, &project, "Secret").await;
    assert_eq!(guessed.status, StatusCode::OK, "{}", guessed.body);
    assert!(
        titles(&guessed).is_empty(),
        "a hidden title matched a search: {}",
        guessed.body
    );
    // Their own titles they may search; both entries still list without a search.
    assert_eq!(
        titles(&search(&guest, &project, "launch").await),
        ["My launch notes"]
    );
    let all = send(&guest, Method::GET, &entries(&project), None, &[]).await;
    assert_eq!(all.body["data"].as_array().map(Vec::len), Some(2));
    // The owner reads every title, so searches every one.
    assert_eq!(titles(&search(&owner, &project, "launch").await).len(), 2);

    // A role that can read no title at all can't search titles: refused, not answered.
    let edited = send(
        &owner,
        Method::PUT,
        &format!("{}/body-reader", roles(&project)),
        Some(&json!({
            "name": "Body reader",
            "permissions": [{"action": "content.read", "subject": "articles", "fields": ["body"]}]
        })),
        &[("if-match", "\"v1\"")],
    )
    .await;
    assert_eq!(edited.status, StatusCode::OK, "{}", edited.body);
    let refused = search(&guest, &project, "launch").await;
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);
    assert_eq!(
        refused.body["error"]["details"]["reason"],
        "titles_not_readable"
    );
}
