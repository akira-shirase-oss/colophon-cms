//! Undoing a takedown in one request (D-184): `POST {P}/releases/{N}/undo` puts back exactly the
//! revisions release N took down, as the next release, or nothing. It is refused, with a named
//! reason, unless N is the newest release, only took entries down, was published by the caller
//! (a person, never a token) under a minute ago, and publishing needs no other reviewer here. Two
//! racing requests can't both commit, a key replays only its own caller's answer, and a gated
//! entry comes back gated. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    ORIGIN, Reply, Server, Session, app, article, call, entries, join, key, project, release,
    sign_in, unique_slug,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

fn base(project: &str) -> String {
    format!("/api/v1/projects/{project}/environments/production")
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

fn reason(reply: &Reply) -> &Value {
    &reply.body["error"]["details"]["reason"]
}

/// `POST …/releases/{generation}/undo` with `idempotency`.
async fn undo(session: &Session, project: &str, generation: &str, idempotency: &str) -> Reply {
    send(
        session,
        Method::POST,
        &format!("{}/releases/{generation}/undo", base(project)),
        None,
        &[("idempotency-key", idempotency)],
    )
    .await
}

/// A new article; its id.
async fn new_article(session: &Session, project: &str, body: &Value) -> String {
    let made = send(
        session,
        Method::POST,
        &entries(project),
        Some(body),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    made.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// Saves `body` over the entry's draft; with `gate`, also sets its visibility.
async fn save(session: &Session, project: &str, entry: &str, body: &Value, gate: Option<&str>) {
    let path = format!("{}/{entry}", entries(project));
    let current = send(session, Method::GET, &path, None, &[]).await;
    let target = gate.map_or(path.clone(), |gate| format!("{path}?visibility={gate}"));
    let saved = send(
        session,
        Method::PUT,
        &target,
        Some(body),
        &[
            ("if-match", &current.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(saved.status, StatusCode::OK, "{}", saved.body);
}

/// Publishes `entry` as the next release; its generation.
async fn publish(session: &Session, project: &str, entry: &str) -> i64 {
    release(
        session,
        project,
        &json!({"title": "Publish", "entries": [entry]}),
    )
    .await
}

/// Takes `entry` down as the next release; its generation.
async fn take_down(session: &Session, project: &str, entry: &str) -> i64 {
    release(
        session,
        project,
        &json!({"title": "Unpublish: it", "unpublish": [entry]}),
    )
    .await
}

/// The public read of one article, anonymous or with `extra` (a visitor's credential): its
/// status and its body text.
async fn read(project: &str, entry: &str, extra: &[(&str, &str)]) -> (StatusCode, String) {
    let mut request = app().get(format!(
        "{ORIGIN}/content/v1/{project}/production/articles/{entry}"
    ));
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("request");
    let status = response.status();
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let text = body["data"]["content"]["body"]["doc"]["content"][0]["content"][0]["text"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    (status, text)
}

/// The environment's newest generation, from the releases list's header.
async fn latest(session: &Session, project: &str) -> i64 {
    let releases = send(
        session,
        Method::GET,
        &format!("{}/releases", base(project)),
        None,
        &[],
    )
    .await;
    releases
        .headers
        .get("colophon-generation")
        .and_then(|value| value.to_str().ok()?.parse().ok())
        .expect("a generation")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: publish, edit, take down, undo, replay, read everything"
)]
async fn an_undo_puts_back_what_readers_had_as_one_release() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let other = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &other, &project, "publisher").await;
    let entry = new_article(
        &owner,
        &project,
        &article("Tide tables", "high water at six"),
    )
    .await;
    assert_eq!(publish(&owner, &project, &entry).await, 1);
    // The draft moves on after publishing; readers still have the first version.
    save(
        &owner,
        &project,
        &entry,
        &article("Tide tables", "high water at seven"),
        None,
    )
    .await;
    assert_eq!(take_down(&owner, &project, &entry).await, 2);
    assert_eq!(read(&project, &entry, &[]).await.0, StatusCode::GONE);

    let undo_key = key();
    let done = undo(&owner, &project, "2", &undo_key).await;
    assert_eq!(done.status, StatusCode::CREATED, "{}", done.body);
    assert_eq!(
        (
            done.body["data"]["generation"].as_i64(),
            done.body["data"]["undo_of"].as_i64(),
            done.body["data"]["entries_restored"].as_i64()
        ),
        (Some(3), Some(2), Some(1)),
        "{}",
        done.body
    );
    assert!(!done.replayed());

    // A double click or a retry: the same key replays the one release; nothing new commits.
    let again = undo(&owner, &project, "2", &undo_key).await;
    assert_eq!(again.status, StatusCode::CREATED, "{}", again.body);
    assert!(again.replayed(), "the second answer is the first, replayed");
    assert_eq!(again.body, done.body);
    assert_eq!(latest(&owner, &project).await, 3);

    // Readers have the version they had, not the newer draft; the draft is untouched.
    assert_eq!(
        read(&project, &entry, &[]).await,
        (StatusCode::OK, "high water at six".to_owned())
    );
    let draft = send(
        &owner,
        Method::GET,
        &format!("{}/{entry}", entries(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(
        draft.body["data"]["content"]["body"]["doc"]["content"][0]["content"][0]["text"],
        "high water at seven",
        "{}",
        draft.body
    );

    // It reads as an undo everywhere: the release, the receipt, its request and the log.
    let releases = send(
        &owner,
        Method::GET,
        &format!("{}/releases", base(&project)),
        None,
        &[],
    )
    .await;
    let newest = &releases.body["data"][0];
    assert_eq!(newest["generation"], 3);
    assert_eq!(newest["undo_of"], 2);
    assert_eq!(newest["title"], "Undo of release 2: Unpublish: it");
    let receipt = send(
        &owner,
        Method::GET,
        &format!("{}/releases/3", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(receipt.body["data"]["undo_of"], 2, "{}", receipt.body);
    let changeset = done.body["data"]["changeset_id"].as_str().expect("an id");
    let request = send(
        &owner,
        Method::GET,
        &format!("{}/changesets/{changeset}", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(
        (
            &request.body["data"]["undo_of"],
            &request.body["data"]["rollback_generation"],
            &request.body["data"]["status"]["state"]
        ),
        (&json!(2), &json!(1), &json!("published")),
        "{}",
        request.body
    );
    let reviews = request.body["data"]["reviews"].as_array().expect("reviews");
    assert_eq!(reviews.len(), 1, "one recorded self-review");
    assert_eq!(reviews[0]["decision"], "approve");
    let log = send(
        &owner,
        Method::GET,
        &format!("/api/v1/projects/{project}/audit"),
        None,
        &[],
    )
    .await;
    let event = log.body["data"]
        .as_array()
        .and_then(|events| {
            events
                .iter()
                .find(|event| event["action"] == "release.undone")
        })
        .expect("the undo is in the audit log");
    assert_eq!(event["subject"], "2");
    assert_eq!(event["detail"]["generation"], 3);
    assert_eq!(event["actor"]["kind"], "person", "{event}");

    // A key is the caller's own: someone else sending it gets no replay of this answer, only
    // their own — release 2 is no longer the newest.
    let borrowed = undo(&other, &project, "2", &undo_key).await;
    assert!(!borrowed.replayed(), "{}", borrowed.body);
    assert_eq!(borrowed.status, StatusCode::CONFLICT, "{}", borrowed.body);
    assert_eq!(reason(&borrowed), "changed_since");
    // And an undo can't itself be undone: it published.
    let twice = undo(&owner, &project, "3", &key()).await;
    assert_eq!(reason(&twice), "not_undoable", "{}", twice.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "three races, told in order: two undos, a publish in between, an undo against a publish"
)]
async fn racing_undos_and_publishes_commit_exactly_one() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let other = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &other, &project, "publisher").await;

    // Two undos of one takedown, with different keys: one commits, the other is told why.
    let first = new_article(&owner, &project, &article("First", "one")).await;
    publish(&owner, &project, &first).await;
    let down = take_down(&owner, &project, &first).await;
    let (taken, key_a, key_b) = (down.to_string(), key(), key());
    let (left, right) = tokio::join!(
        undo(&owner, &project, &taken, &key_a),
        undo(&owner, &project, &taken, &key_b)
    );
    let mut statuses = [left.status, right.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} / {}",
        left.body,
        right.body
    );
    let loser = if left.status == StatusCode::CONFLICT {
        &left
    } else {
        &right
    };
    assert_eq!(reason(loser), "changed_since", "{}", loser.body);
    assert_eq!(latest(&owner, &project).await, down + 1);

    // Someone else publishes between the takedown and the undo: refused, and nothing is put back.
    let second = new_article(&owner, &project, &article("Second", "two")).await;
    publish(&owner, &project, &second).await;
    let down = take_down(&owner, &project, &second).await;
    let third = new_article(&other, &project, &article("Third", "three")).await;
    publish(&other, &project, &third).await;
    let late = undo(&owner, &project, &down.to_string(), &key()).await;
    assert_eq!(late.status, StatusCode::CONFLICT, "{}", late.body);
    assert_eq!(reason(&late), "changed_since");
    assert_eq!(read(&project, &second, &[]).await.0, StatusCode::GONE);

    // An undo racing another person's publish on the same base: exactly one commits.
    let fourth = new_article(&owner, &project, &article("Fourth", "four")).await;
    publish(&owner, &project, &fourth).await;
    let down = take_down(&owner, &project, &fourth).await;
    let fifth = new_article(&other, &project, &article("Fifth", "five")).await;
    let made = send(
        &other,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "Fifth", "entries": [fifth]})),
        &[("idempotency-key", &key())],
    )
    .await;
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        made.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!(
        "\"{}\"",
        made.body["data"]["hash"].as_str().expect("a hash")
    );
    send(
        &other,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await;
    let approved = send(
        &other,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await;
    assert_eq!(
        approved.body["data"]["status"]["state"], "approved",
        "{}",
        approved.body
    );
    let (publish_key, undo_key, taken) = (key(), key(), down.to_string());
    let (publish_path, publish_body) =
        (format!("{path}/publish"), json!({"base_generation": down}));
    let publish_headers = [
        ("if-match", if_match.as_str()),
        ("idempotency-key", publish_key.as_str()),
    ];
    let (undone, published) = tokio::join!(
        undo(&owner, &project, &taken, &undo_key),
        send(
            &other,
            Method::POST,
            &publish_path,
            Some(&publish_body),
            &publish_headers
        )
    );
    let mut statuses = [undone.status, published.status];
    statuses.sort();
    assert_eq!(
        statuses,
        [StatusCode::CREATED, StatusCode::CONFLICT],
        "{} / {}",
        undone.body,
        published.body
    );
    let refused = if undone.status == StatusCode::CONFLICT {
        reason(&undone).clone()
    } else {
        reason(&published).clone()
    };
    assert!(
        refused == "changed_since" || refused == "generation_moved",
        "{refused}"
    );
    assert_eq!(latest(&owner, &project).await, down + 1);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "every refusal, each on its own takedown, in the order the rules check them"
)]
async fn an_undo_is_refused_with_a_named_reason() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let other = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &other, &project, "publisher").await;
    let entry = new_article(&owner, &project, &article("Refusals", "text")).await;
    publish(&owner, &project, &entry).await;
    let down = take_down(&owner, &project, &entry).await.to_string();

    // The request's own shape: a key is required, and the release must exist.
    let no_key = send(
        &owner,
        Method::POST,
        &format!("{}/releases/{down}/undo", base(&project)),
        None,
        &[],
    )
    .await;
    assert_eq!(
        no_key.status,
        StatusCode::PRECONDITION_REQUIRED,
        "{}",
        no_key.body
    );
    for generation in ["0", "-1", "99"] {
        let reply = undo(&owner, &project, generation, &key()).await;
        assert_eq!(
            (reply.status, reason(&reply).clone()),
            (
                StatusCode::UNPROCESSABLE_ENTITY,
                json!("invalid_generation")
            ),
            "{generation}: {}",
            reply.body
        );
    }
    let malformed = undo(&owner, &project, "two", &key()).await;
    assert!(malformed.status.is_client_error(), "{}", malformed.body);
    assert!(
        malformed.body["error"]["code"].is_string(),
        "{}",
        malformed.body
    );

    // Not the person who published it — another publisher, an editor, a reader, a token.
    let reply = undo(&other, &project, &down, &key()).await;
    assert_eq!(
        (reply.status, reason(&reply).clone()),
        (StatusCode::FORBIDDEN, json!("not_your_release"))
    );
    for role in ["editor", "reader"] {
        let guest = sign_in("dev-editor").await;
        let shared = common::project(&owner).await;
        if role == "reader" {
            // A custom role that may only read articles.
            let made = send(
                &owner,
                Method::POST,
                &format!("/api/v1/projects/{shared}/roles"),
                Some(&json!({
                    "key": "reader",
                    "name": "Reader",
                    "permissions": [{"action": "content.read", "subject": "articles"}]
                })),
                &[],
            )
            .await;
            assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        }
        join(&owner, &guest, &shared, role).await;
        let gone = new_article(&owner, &shared, &article("Theirs", "text")).await;
        publish(&owner, &shared, &gone).await;
        let taken = take_down(&owner, &shared, &gone).await.to_string();
        let reply = undo(&guest, &shared, &taken, &key()).await;
        assert_eq!(
            reply.status,
            StatusCode::FORBIDDEN,
            "{role}: {}",
            reply.body
        );
        assert_eq!(
            read(&shared, &gone, &[]).await.0,
            StatusCode::GONE,
            "{role}"
        );
    }
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/tokens"),
        Some(&json!({"name": "Agent", "kind": "full-access", "expires_in_days": 1})),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let secret = made.body["data"]["secret"].as_str().expect("a secret");
    let by_token = app()
        .post(format!("{ORIGIN}{}/releases/{down}/undo", base(&project)))
        .header(header::AUTHORIZATION, format!("Bearer {secret}"))
        .header("idempotency-key", key())
        .send()
        .await
        .expect("request");
    assert_eq!(by_token.status(), StatusCode::FORBIDDEN);
    let body: Value = by_token.json().await.expect("json");
    assert_eq!(
        body["error"]["details"]["reason"], "agent_self_review",
        "{body}"
    );
    assert_eq!(read(&project, &entry, &[]).await.0, StatusCode::GONE);

    // Too late, by the database's clock: the release's stored time, a minute and a bit ago.
    let database = common::owner_database().await;
    sqlx::query(
        "UPDATE releases SET published_at = now() - interval '61 seconds'
         WHERE project_id = $1::uuid AND generation = $2",
    )
    .bind(&project)
    .bind(down.parse::<i64>().expect("a number"))
    .execute(&database)
    .await
    .expect("age the release");
    let reply = undo(&owner, &project, &down, &key()).await;
    assert_eq!(
        (reply.status, reason(&reply).clone()),
        (StatusCode::CONFLICT, json!("undo_expired")),
        "{}",
        reply.body
    );

    // Deleted since the takedown (D-080): it can't come back.
    let deleted = new_article(&owner, &project, &article("Deleted", "text")).await;
    publish(&owner, &project, &deleted).await;
    let down = take_down(&owner, &project, &deleted).await.to_string();
    let path = format!("{}/{deleted}", entries(&project));
    let current = send(&owner, Method::GET, &path, None, &[]).await;
    let gone = send(
        &owner,
        Method::DELETE,
        &path,
        None,
        &[
            ("if-match", &current.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert!(gone.status.is_success(), "{}", gone.body);
    let reply = undo(&owner, &project, &down, &key()).await;
    assert_eq!(
        (reply.status, reason(&reply).clone()),
        (StatusCode::CONFLICT, json!("entry_deleted")),
        "{}",
        reply.body
    );

    // A slug or a unique value can't be taken while its entry lives, taken down or not, so an
    // undo never meets a clash: the draft that tries is refused by name, and the undo goes through.
    let defined = send(
        &owner,
        Method::POST,
        &format!("{}/content-types", base(&project)),
        Some(&json!({"slug": "pages", "definition": {"name": "Pages", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "slug", "label": "URL name", "type": "slug", "source": "title"},
            {"id": "code", "label": "Code", "type": "text", "unique": true}
        ]}})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(defined.status, StatusCode::CREATED, "{}", defined.body);
    let pages = format!("{}/collections/pages/entries", base(&project));
    let slug = format!("tides-{}", unique_slug());
    let named = send(
        &owner,
        Method::POST,
        &pages,
        Some(&json!({"title": "Tides", "slug": slug, "code": "T1"})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(named.status, StatusCode::CREATED, "{}", named.body);
    let page = named.body["data"]["id"].as_str().expect("an id").to_owned();
    publish(&owner, &project, &page).await;
    let down = take_down(&owner, &project, &page).await.to_string();
    for (copy, refused) in [
        (json!({"title": "Copy", "slug": slug}), "slug_taken"),
        (json!({"title": "Copy", "code": "T1"}), "value_taken"),
    ] {
        let clash = send(
            &owner,
            Method::POST,
            &pages,
            Some(&copy),
            &[("idempotency-key", &key())],
        )
        .await;
        assert_eq!(reason(&clash), refused, "{}", clash.body);
    }
    let reply = undo(&owner, &project, &down, &key()).await;
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);

    // Review stages configured after the takedown: the undo would approve itself, so it is a
    // request for review instead, and nothing comes back.
    let staged = new_article(&owner, &project, &article("Staged", "text")).await;
    publish(&owner, &project, &staged).await;
    let down = take_down(&owner, &project, &staged).await.to_string();
    let review_stages = format!("{}/review-stages", base(&project));
    let empty = send(&owner, Method::GET, &review_stages, None, &[]).await;
    let configured = send(
        &owner,
        Method::PUT,
        &review_stages,
        Some(&json!({"stages": [
            {"key": "editorial", "name": "Editorial"},
            {"key": "legal", "name": "Legal", "different_reviewer": true}
        ]})),
        &[
            ("if-match", &empty.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await;
    assert_eq!(configured.status, StatusCode::OK, "{}", configured.body);
    let reply = undo(&owner, &project, &down, &key()).await;
    assert_eq!(
        (reply.status, reason(&reply).clone()),
        (StatusCode::CONFLICT, json!("needs_review")),
        "{}",
        reply.body
    );
    assert_eq!(read(&project, &staged, &[]).await.0, StatusCode::GONE);

    // Nothing that was refused left a request or a release behind.
    let releases = latest(&owner, &project).await;
    assert_eq!(releases, down.parse::<i64>().expect("a number"));
}

/// The cookies a visitor's signup answers with, as one `Cookie` header value.
async fn visitor(project: &str) -> String {
    let response = app()
        .post(format!("{ORIGIN}/audience/v1/{project}/signup"))
        .json(&json!({
            "email": format!("visitor-{}@example.test", unique_slug()),
            "password": "a-long-enough-password",
            "display_name": "Visitor"
        }))
        .send()
        .await
        .expect("request");
    assert_eq!(response.status(), StatusCode::CREATED);
    let body: Value = response.json().await.expect("json");
    format!(
        "Bearer {}",
        body["data"]["session"].as_str().expect("a session token")
    )
}

/// D-183's class of bug, for undo: a gated entry taken down and put back is gated again — never
/// public — even when its draft was made public in between. Read through public delivery as
/// anonymous and as a member, before and after.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_gated_entry_comes_back_gated() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let made = send(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{project}/audience/roles"),
        Some(&json!({"name": "member", "self_assign": true})),
        &[],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let member = visitor(&project).await;
    let as_member = [("authorization", member.as_str())];
    let mut gated = Vec::new();
    for gate in ["role:member", "authenticated"] {
        let made = send(
            &owner,
            Method::POST,
            &format!("{}?visibility={gate}", entries(&project)),
            Some(&article("Members only", "for members")),
            &[("idempotency-key", &key())],
        )
        .await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        gated.push(made.body["data"]["id"].as_str().expect("an id").to_owned());
    }
    for entry in &gated {
        publish(&owner, &project, entry).await;
        assert_eq!(read(&project, entry, &as_member).await.0, StatusCode::OK);
        assert_eq!(read(&project, entry, &[]).await.0, StatusCode::NOT_FOUND);

        let down = take_down(&owner, &project, entry).await;
        assert_eq!(read(&project, entry, &as_member).await.0, StatusCode::GONE);
        assert_ne!(read(&project, entry, &[]).await.0, StatusCode::OK);
        // The draft is made public meanwhile; the undo puts back what readers had, gate and all.
        save(
            &owner,
            &project,
            entry,
            &article("Now public", "for everyone"),
            Some("public"),
        )
        .await;

        let done = undo(&owner, &project, &down.to_string(), &key()).await;
        assert_eq!(done.status, StatusCode::CREATED, "{}", done.body);
        assert_eq!(
            read(&project, entry, &as_member).await,
            (StatusCode::OK, "for members".to_owned())
        );
        assert_eq!(read(&project, entry, &[]).await.0, StatusCode::NOT_FOUND);
    }
    // Nor does an anonymous list read show them.
    let list = app()
        .get(format!("{ORIGIN}/content/v1/{project}/production/articles"))
        .send()
        .await
        .expect("request");
    let body: Value = list.json().await.expect("json");
    assert_eq!(body["data"], json!([]), "{body}");
}

/// An undo is a publish, so it shares publishing's idempotency scope (D-184): one key names one
/// request, and sending it for the other is refused, either way round.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_key_used_for_a_publish_cannot_name_an_undo_or_the_reverse() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let entry = new_article(&owner, &project, &article("Keys", "text")).await;
    publish(&owner, &project, &entry).await;
    // A takedown published with a key of our own, then an undo sent with that same key.
    let made = send(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "Down", "unpublish": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await;
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        made.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!(
        "\"{}\"",
        made.body["data"]["hash"].as_str().expect("a hash")
    );
    send(
        &owner,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await;
    send(
        &owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await;
    let shared = key();
    let published = send(
        &owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": 1})),
        &[("if-match", &if_match), ("idempotency-key", &shared)],
    )
    .await;
    assert_eq!(published.status, StatusCode::CREATED, "{}", published.body);
    let reused = undo(&owner, &project, "2", &shared).await;
    assert_eq!(reason(&reused), "idempotency_key_reused", "{}", reused.body);
    assert!(!reused.replayed());
    // The reverse: an undo's key, then a publish sent with it.
    let undo_key = key();
    assert_eq!(
        undo(&owner, &project, "2", &undo_key).await.status,
        StatusCode::CREATED
    );
    let other = new_article(&owner, &project, &article("Other", "text")).await;
    let made = send(
        &owner,
        Method::POST,
        &format!("{}/changesets", base(&project)),
        Some(&json!({"title": "Other", "entries": [other]})),
        &[("idempotency-key", &key())],
    )
    .await;
    let path = format!(
        "{}/changesets/{}",
        base(&project),
        made.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!(
        "\"{}\"",
        made.body["data"]["hash"].as_str().expect("a hash")
    );
    send(
        &owner,
        Method::POST,
        &format!("{path}/submit"),
        None,
        &[("if-match", &if_match)],
    )
    .await;
    send(
        &owner,
        Method::POST,
        &format!("{path}/reviews"),
        Some(&json!({"decision": "approve"})),
        &[("if-match", &if_match)],
    )
    .await;
    let refused = send(
        &owner,
        Method::POST,
        &format!("{path}/publish"),
        Some(&json!({"base_generation": 3})),
        &[("if-match", &if_match), ("idempotency-key", &undo_key)],
    )
    .await;
    assert_eq!(
        reason(&refused),
        "idempotency_key_reused",
        "{}",
        refused.body
    );
    assert_eq!(latest(&owner, &project).await, 3);
}

/// The lock order, deterministically (D-131, D-135): an undo takes publishing's admission pair
/// before any lock on the environment row. A third session holding the project's admission lock
/// makes the undo wait — and while it waits, that session can still lock the environment row
/// without waiting (`NOWAIT`), which it couldn't if the undo had taken the row first. Reordering
/// the undo's locks fails here every time, not one race in a hundred.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_undo_waits_for_the_admission_lock_before_touching_the_environment_row() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let entry = new_article(&owner, &project, &article("Locks", "text")).await;
    publish(&owner, &project, &entry).await;
    let down = take_down(&owner, &project, &entry).await;

    let database = common::owner_database().await;
    let mut third = database.begin().await.expect("a transaction");
    // `colophon_db::work::lock`'s key: class "WA", the project id's text hash.
    sqlx::query("SELECT pg_advisory_xact_lock(22337, hashtext($1::text))")
        .bind(&project)
        .execute(&mut *third)
        .await
        .expect("hold the admission lock");
    let waiting = tokio::spawn({
        let owner = owner.clone();
        let project = project.clone();
        async move { undo(&owner, &project, &down.to_string(), &key()).await }
    });
    tokio::time::sleep(std::time::Duration::from_millis(1500)).await;
    assert!(
        !waiting.is_finished(),
        "the undo didn't wait for the admission lock"
    );
    let row = sqlx::query(
        "SELECT 1 FROM environments WHERE project_id = $1::uuid FOR NO KEY UPDATE NOWAIT",
    )
    .bind(&project)
    .execute(&mut *third)
    .await;
    assert!(
        row.is_ok(),
        "the waiting undo already holds a lock on the environment row: {row:?}"
    );
    third.commit().await.expect("release");
    let done = waiting.await.expect("the undo task");
    assert_eq!(done.status, StatusCode::CREATED, "{}", done.body);
}

/// Races, repeated (`UNDO_RACE_ROUNDS`; by default 200 in CI, where `CI` is set, and 20 locally): two undos of one takedown, and an undo
/// against another person's publish on the same base. Every round has exactly one winner and the
/// loser is refused by name — never a 500 or a deadlock. The counts are printed.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "two races in one loop, counted the same way"
)]
async fn races_repeated_always_have_one_winner_and_a_named_loser() {
    let rounds: usize = std::env::var("UNDO_RACE_ROUNDS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(if std::env::var_os("CI").is_some() {
            200
        } else {
            20
        });
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let other = sign_in("dev-editor").await;
    let project = project(&owner).await;
    join(&owner, &other, &project, "publisher").await;
    let entry = new_article(&owner, &project, &article("Raced", "text")).await;
    publish(&owner, &project, &entry).await;
    let mut tally: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut note = |race: &str, reply: &Reply| {
        let what = if reply.status == StatusCode::CREATED {
            "won".to_owned()
        } else {
            format!(
                "{} {}",
                reply.status.as_u16(),
                reason(reply).as_str().unwrap_or("-")
            )
        };
        *tally.entry(format!("{race}: {what}")).or_default() += 1;
    };
    for round in 0..rounds {
        // Undo against undo.
        let down = take_down(&owner, &project, &entry).await;
        let (taken, a, b) = (down.to_string(), key(), key());
        let (left, right) = tokio::join!(
            undo(&owner, &project, &taken, &a),
            undo(&owner, &project, &taken, &b)
        );
        note("undo vs undo", &left);
        note("undo vs undo", &right);
        let wins = [&left, &right]
            .iter()
            .filter(|r| r.status == StatusCode::CREATED)
            .count();
        assert_eq!(wins, 1, "round {round}: {} / {}", left.body, right.body);

        // Undo against another person's publish on the same base.
        let down = take_down(&owner, &project, &entry).await;
        let fresh = new_article(
            &other,
            &project,
            &article(&format!("Fresh {round}"), "text"),
        )
        .await;
        let made = send(
            &other,
            Method::POST,
            &format!("{}/changesets", base(&project)),
            Some(&json!({"title": "Fresh", "entries": [fresh]})),
            &[("idempotency-key", &key())],
        )
        .await;
        let path = format!(
            "{}/changesets/{}",
            base(&project),
            made.body["data"]["id"].as_str().expect("an id")
        );
        let if_match = format!(
            "\"{}\"",
            made.body["data"]["hash"].as_str().expect("a hash")
        );
        send(
            &other,
            Method::POST,
            &format!("{path}/submit"),
            None,
            &[("if-match", &if_match)],
        )
        .await;
        send(
            &other,
            Method::POST,
            &format!("{path}/reviews"),
            Some(&json!({"decision": "approve"})),
            &[("if-match", &if_match)],
        )
        .await;
        let (taken, undo_key, publish_key) = (down.to_string(), key(), key());
        let (publish_path, publish_body) =
            (format!("{path}/publish"), json!({"base_generation": down}));
        let publish_headers = [
            ("if-match", if_match.as_str()),
            ("idempotency-key", publish_key.as_str()),
        ];
        let (undone, published) = tokio::join!(
            undo(&owner, &project, &taken, &undo_key),
            send(
                &other,
                Method::POST,
                &publish_path,
                Some(&publish_body),
                &publish_headers
            )
        );
        note("undo vs publish (undo)", &undone);
        note("undo vs publish (publish)", &published);
        let wins = [&undone, &published]
            .iter()
            .filter(|r| r.status == StatusCode::CREATED)
            .count();
        assert_eq!(
            wins, 1,
            "round {round}: {} / {}",
            undone.body, published.body
        );
        // Whichever lost, the entry must be live for the next round's takedown.
        if undone.status != StatusCode::CREATED {
            publish(&owner, &project, &entry).await;
        }
    }
    println!("{rounds} rounds: {tally:?}");
    for (outcome, count) in &tally {
        assert!(
            outcome.ends_with("won")
                || outcome.ends_with("409 changed_since")
                || outcome.ends_with("409 generation_moved"),
            "{outcome} x{count}"
        );
    }
}
