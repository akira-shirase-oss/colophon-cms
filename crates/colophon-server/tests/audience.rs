//! The audience over HTTP (docs/modules/03-auth.md, "Audience"; D-137): a project's visitors
//! sign up and in on `/audience/v1`, staff manage their roles and accounts under
//! `audience.manage`, and delivery answers what the caller may read — anonymous getting only
//! `public` from the shared cache, a member getting their gate from Postgres every time,
//! marked `private` so nothing shared keeps it. Needs the stack and `scripts/migrate.sh`.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{
    ORIGIN, PORT, Server, app, article, call, cookie, cookies_from, create_article, entries, join,
    key, project, release, sign_in, unique_slug,
};
use reqwest::{Method, StatusCode, header};
use serde_json::{Value, json};

const MEMBER_CACHE: &str = "private, no-cache";

/// `POST /audience/v1/{project}/…` has no caller cookies: a site's own pages are the client.
async fn audience_post(project: &str, action: &str, body: &Value) -> reqwest::Response {
    app()
        .post(format!("{ORIGIN}/audience/v1/{project}/{action}"))
        .json(body)
        .send()
        .await
        .expect("request")
}

/// `POST {base}/audience/v1/{project}/{action}` against a named server: `audience_post`
/// always hits [`ORIGIN`], a second server needs its own.
async fn audience_post_at(
    base: &str,
    project: &str,
    action: &str,
    body: &Value,
) -> reqwest::Response {
    app()
        .post(format!("{base}/audience/v1/{project}/{action}"))
        .json(body)
        .send()
        .await
        .expect("request")
}

/// The token a `dev_link` carries: `return_to` with `?token=` appended (or `&` when the
/// address already has a query).
fn dev_token(link: &str) -> &str {
    link.rsplit("token=")
        .next()
        .filter(|token| !token.is_empty() && *token != link)
        .expect("a ?token= on the development link")
}

/// `GET /audience/v1/{project}/me` with whatever headers the caller carries.
async fn me(project: &str, extra: &[(&str, &str)]) -> (StatusCode, Value) {
    let mut request = app().get(format!("{ORIGIN}/audience/v1/{project}/me"));
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("request");
    (
        response.status(),
        response.json().await.unwrap_or(Value::Null),
    )
}

/// A delivery list read, with the headers a member read sets. `extra` carries the audience
/// credential — a `Cookie` or an `Authorization` header.
struct Delivery {
    status: StatusCode,
    cache: String,
    cache_control: String,
    vary: String,
    titles: Vec<String>,
}

async fn delivered(project: &str, extra: &[(&str, &str)]) -> Delivery {
    let mut request = app().get(format!("{ORIGIN}/content/v1/{project}/production/articles"));
    for (name, value) in extra {
        request = request.header(*name, *value);
    }
    let response = request.send().await.expect("request");
    let text = |name: &str| {
        response
            .headers()
            .get(name)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_owned()
    };
    let (status, cache, cache_control, vary) = (
        response.status(),
        text("colophon-cache"),
        text("cache-control"),
        text("vary"),
    );
    let body: Value = response.json().await.unwrap_or(Value::Null);
    let mut titles: Vec<String> = body["data"]
        .as_array()
        .map(|rows| {
            rows.iter()
                .filter_map(|row| row["title"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default();
    titles.sort();
    Delivery {
        status,
        cache,
        cache_control,
        vary,
        titles,
    }
}

/// An entry created gated as `visibility` (`?visibility=`); its id.
async fn create_gated(
    owner: &common::Session,
    project: &str,
    title: &str,
    visibility: &str,
) -> String {
    let reply = call(
        owner,
        Method::POST,
        &format!("{}?visibility={visibility}", entries(project)),
        Some(&article(title, "text")),
        &[("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(reply.status, StatusCode::CREATED, "{}", reply.body);
    reply.body["data"]["id"].as_str().expect("an id").to_owned()
}

/// The cookies `signin`/`signup` answer with, as one `Cookie` header value, and the CSRF
/// token writes echo.
fn audience_cookie(response: &reqwest::Response) -> (String, String) {
    let set = cookies_from(response);
    let (session, csrf) = (
        cookie(&set, "colophon_audience"),
        cookie(&set, "colophon_audience_csrf"),
    );
    assert_eq!((session.len(), csrf.len()), (43, 43), "{set:?}");
    (
        format!("colophon_audience={session}; colophon_audience_csrf={csrf}"),
        csrf,
    )
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey, told in order: signup, gated delivery, cookie and bearer, signout"
)]
async fn visitors_sign_up_sign_in_and_read_only_what_their_gate_allows() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let staff = format!("/api/v1/projects/{project}/audience");

    // The project's audience roles: `member` open to signup, `vip` staff-only.
    for (name, open) in [("member", true), ("vip", false)] {
        let made = call(
            &owner,
            Method::POST,
            &format!("{staff}/roles"),
            Some(&json!({"name": name, "self_assign": open})),
            &[],
        )
        .await
        .expect("request");
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }

    // Four gates, published in one release.
    let entries = [
        create_gated(&owner, &project, "For everyone", "public").await,
        create_gated(&owner, &project, "For accounts", "authenticated").await,
        create_gated(&owner, &project, "For members", "role:member").await,
        create_gated(&owner, &project, "For vips", "role:vip").await,
    ];
    release(
        &owner,
        &project,
        &json!({"title": "Gates", "entries": entries}),
    )
    .await;

    // Anonymous reads the public row alone — the database hides the rest, so a single
    // read of a gated entry can't be told apart from one that never published (404).
    let anon = delivered(&project, &[]).await;
    assert_eq!(anon.status, StatusCode::OK);
    assert_eq!(anon.titles, ["For everyone"]);
    let gated = app()
        .get(format!(
            "{ORIGIN}/content/v1/{project}/production/articles/{}",
            entries[2]
        ))
        .send()
        .await
        .expect("request");
    assert_eq!(gated.status(), StatusCode::NOT_FOUND);

    // Signup takes the project's single open role; the answer carries the session token
    // and the cookies carry it for browsers.
    let email = format!("visitor-{}@example.test", unique_slug());
    let password = "a-long-enough-password";
    let response = audience_post(
        &project,
        "signup",
        &json!({"email": email, "password": password, "display_name": "Visitor"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::CREATED);
    let (cookies, csrf) = audience_cookie(&response);
    let body: Value = response.json().await.expect("json");
    let token = body["data"]["session"]
        .as_str()
        .expect("a token")
        .to_owned();
    assert_eq!(body["data"]["user"]["role"].as_str(), Some("member"));

    // `me` knows the cookie and the bearer alike; nobody is nobody.
    let (status, _) = me(&project, &[("cookie", cookies.as_str())]).await;
    assert_eq!(status, StatusCode::OK);
    let (status, body) = me(&project, &[("authorization", &format!("Bearer {token}"))]).await;
    assert_eq!(
        (status, body["data"]["email"].as_str()),
        (StatusCode::OK, Some(email.as_str()))
    );
    let (status, _) = me(&project, &[]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A member's read: their gate, from Postgres every time — never the shared cache,
    // and marked so no shared cache keeps it.
    let member = delivered(&project, &[("authorization", &format!("Bearer {token}"))]).await;
    assert_eq!(member.status, StatusCode::OK);
    assert_eq!(
        member.titles,
        ["For accounts", "For everyone", "For members"]
    );
    assert_eq!(
        (member.cache.as_str(), member.cache_control.as_str()),
        ("bypass", MEMBER_CACHE)
    );
    assert!(member.vary.contains("Authorization"), "{}", member.vary);

    // The cookie carries the same session to the same read.
    let via_cookie = delivered(&project, &[("cookie", cookies.as_str())]).await;
    assert_eq!(via_cookie.titles, member.titles);

    // A member read never entered the anonymous cache: still the public row alone.
    let anon_after = delivered(&project, &[]).await;
    assert_eq!(anon_after.titles, ["For everyone"]);

    // A cookie-carried signout wants the CSRF double-submit, like a staff session's writes.
    for (with_csrf, expected) in [
        (false, StatusCode::FORBIDDEN),
        (true, StatusCode::NO_CONTENT),
    ] {
        let mut request = app()
            .post(format!("{ORIGIN}/audience/v1/{project}/signout"))
            .header(header::COOKIE, &cookies);
        if with_csrf {
            request = request.header("x-csrf-token", &csrf);
        }
        assert_eq!(request.send().await.expect("request").status(), expected);
    }
    let (status, _) = me(&project, &[("cookie", cookies.as_str())]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // The bearer form needs nothing ambient — a token is never sent by accident.
    let response = audience_post(
        &project,
        "signin",
        &json!({"email": email, "password": password}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    let token = body["data"]["session"]
        .as_str()
        .expect("a token")
        .to_owned();
    let signout = app()
        .post(format!("{ORIGIN}/audience/v1/{project}/signout"))
        .header(header::AUTHORIZATION, format!("Bearer {token}"))
        .send()
        .await
        .expect("request");
    assert_eq!(signout.status(), StatusCode::NO_CONTENT);
    let (status, _) = me(&project, &[("authorization", &format!("Bearer {token}"))]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);

    // A wrong password and no account answer alike.
    for (email, reason) in [
        (email.as_str(), "wrong-password"),
        ("nobody@example.test", password),
    ] {
        let response = audience_post(
            &project,
            "signin",
            &json!({"email": email, "password": reason}),
        )
        .await;
        // `sign_in_failed` is a 400 like the staff sign-in's — wrong password and no
        // account share the one answer.
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
        let body: Value = response.json().await.expect("json");
        assert_eq!(
            body["error"]["code"].as_str(),
            Some("sign_in_failed"),
            "{body}"
        );
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey, told in order: roles, a seated user, sessions, disable, revoke"
)]
async fn staff_manage_roles_users_and_sessions_under_audience_manage() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let staff = format!("/api/v1/projects/{project}/audience");

    // `audience.manage` is an owner's action: an editor is refused.
    let editor = sign_in("dev-editor").await;
    join(&owner, &editor, &project, "editor").await;
    let refused = call(&editor, Method::GET, &format!("{staff}/roles"), None, &[])
        .await
        .expect("request");
    assert_eq!(refused.status, StatusCode::FORBIDDEN, "{}", refused.body);

    // A role name is a label other systems carry: the same shape a member role's key has.
    let bad = call(
        &owner,
        Method::POST,
        &format!("{staff}/roles"),
        Some(&json!({"name": "Not A Role"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        bad.body["error"]["details"]["reason"].as_str(),
        Some("invalid_role"),
        "{:?}",
        bad.body
    );
    for (name, open) in [("member", true), ("vip", false)] {
        let made = call(
            &owner,
            Method::POST,
            &format!("{staff}/roles"),
            Some(&json!({"name": name, "self_assign": open})),
            &[],
        )
        .await
        .expect("request");
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
        assert_eq!(
            (
                made.body["data"]["name"].as_str(),
                made.body["data"]["self_assign"].as_bool()
            ),
            (Some(name), Some(open))
        );
    }
    // Creating is not idempotent at the API: the same name answers the conflict.
    let again = call(
        &owner,
        Method::POST,
        &format!("{staff}/roles"),
        Some(&json!({"name": "member"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        again.body["error"]["details"]["reason"].as_str(),
        Some("role_exists"),
        "{:?}",
        again.body
    );

    // Signup may stop offering a role without deleting it.
    let closed = call(
        &owner,
        Method::PATCH,
        &format!("{staff}/roles/member"),
        Some(&json!({"self_assign": false})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        closed.body["data"]["self_assign"].as_bool(),
        Some(false),
        "{:?}",
        closed.body
    );
    call(
        &owner,
        Method::PATCH,
        &format!("{staff}/roles/member"),
        Some(&json!({"self_assign": true})),
        &[],
    )
    .await
    .expect("request");

    // Staff make a site user in any role — closed ones too — and the mistakes are named.
    let unknown = call(
        &owner,
        Method::POST,
        &format!("{staff}/users"),
        Some(&json!({"email": "a@b.c", "password": "a-long-enough-password", "role": "ghost"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        unknown.body["error"]["details"]["reason"].as_str(),
        Some("unknown_audience_role"),
        "{:?}",
        unknown.body
    );
    let email = format!("vip-{}@example.test", unique_slug());
    let made = call(
        &owner,
        Method::POST,
        &format!("{staff}/users"),
        Some(&json!({"email": email, "password": "a-long-enough-password", "role": "vip"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let user = made.body["data"]["id"].as_str().expect("an id").to_owned();
    assert_eq!(made.body["data"]["role"].as_str(), Some("vip"));

    // The user signs in and reads the closed role's gate.
    let entry = create_gated(&owner, &project, "For vips", "role:vip").await;
    release(
        &owner,
        &project,
        &json!({"title": "Vip", "entries": [entry]}),
    )
    .await;
    let response = audience_post(
        &project,
        "signin",
        &json!({"email": email, "password": "a-long-enough-password"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::OK);
    let body: Value = response.json().await.expect("json");
    let token = body["data"]["session"]
        .as_str()
        .expect("a token")
        .to_owned();
    let vip = delivered(&project, &[("authorization", &format!("Bearer {token}"))]).await;
    assert_eq!(vip.titles, ["For vips"]);

    // An empty change is a named mistake; a real one is recorded.
    let empty = call(
        &owner,
        Method::PATCH,
        &format!("{staff}/users/{user}"),
        Some(&json!({})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        empty.body["error"]["details"]["reason"].as_str(),
        Some("empty_change"),
        "{:?}",
        empty.body
    );
    let moved = call(
        &owner,
        Method::PATCH,
        &format!("{staff}/users/{user}"),
        Some(&json!({"role": "member"})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(moved.body["data"]["role"].as_str(), Some("member"));

    // A role users hold can't be deleted; once they don't, it can.
    let held = call(
        &owner,
        Method::DELETE,
        &format!("{staff}/roles/member"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(
        held.body["error"]["details"]["reason"].as_str(),
        Some("role_in_use"),
        "{:?}",
        held.body
    );

    // A disabled account's sessions die with it: the bearer reads anonymous at once,
    // and signing back in says so.
    let disabled = call(
        &owner,
        Method::PATCH,
        &format!("{staff}/users/{user}"),
        Some(&json!({"disabled": true})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(disabled.body["data"]["disabled"].as_bool(), Some(true));
    let (status, _) = me(&project, &[("authorization", &format!("Bearer {token}"))]).await;
    assert_eq!(status, StatusCode::UNAUTHORIZED);
    let response = audience_post(
        &project,
        "signin",
        &json!({"email": email, "password": "a-long-enough-password"}),
    )
    .await;
    assert_eq!(response.status(), StatusCode::FORBIDDEN);
    let body: Value = response.json().await.expect("json");
    assert_eq!(
        body["error"]["details"]["reason"].as_str(),
        Some("account_disabled"),
        "{body}"
    );

    // Staff see the user, the user's sessions (live and dead), and revoke one.
    let list = call(&owner, Method::GET, &format!("{staff}/users"), None, &[])
        .await
        .expect("request");
    assert!(
        list.body["data"].as_array().is_some_and(|users| users
            .iter()
            .any(|row| row["id"].as_str() == Some(user.as_str()))),
        "{:?}",
        list.body
    );
    let sessions = call(
        &owner,
        Method::GET,
        &format!("{staff}/users/{user}/sessions"),
        None,
        &[],
    )
    .await
    .expect("request");
    let session = sessions.body["data"]
        .as_array()
        .and_then(|rows| rows.first())
        .and_then(|row| row["id"].as_str())
        .expect("a session")
        .to_owned();
    assert_eq!(
        sessions.body["data"]
            .as_array()
            .and_then(|r| r.first())
            .and_then(|r| r["revoked"].as_bool()),
        Some(true),
        "the disable revoked it"
    );
    let gone = call(
        &owner,
        Method::DELETE,
        &format!("{staff}/sessions/{session}"),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(gone.status, StatusCode::NOT_FOUND, "already revoked");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn a_write_names_its_gate_and_keeps_it_without_being_asked() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let staff = format!("/api/v1/projects/{project}/audience");
    let made = call(
        &owner,
        Method::POST,
        &format!("{staff}/roles"),
        Some(&json!({"name": "member", "self_assign": true})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);

    // The gate is checked on the write: a bad value and a role that isn't are both named.
    for (gate, reason) in [
        ("everyone", "invalid_visibility"),
        ("role:ghost", "unknown_audience_role"),
    ] {
        let refused = call(
            &owner,
            Method::POST,
            &format!("{}?visibility={gate}", entries(&project)),
            Some(&article("Gated", "text")),
            &[("idempotency-key", &key())],
        )
        .await
        .expect("request");
        assert_eq!(
            refused.body["error"]["details"]["reason"].as_str(),
            Some(reason),
            "{gate}: {:?}",
            refused.body
        );
    }

    // Created gated, saved without `?visibility=`: the gate stays — an edit is not a leak.
    let id = create_gated(&owner, &project, "Gated", "role:member").await;
    let current = call(
        &owner,
        Method::GET,
        &format!("{}/{id}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    let etag = current.etag().expect("an ETag");
    assert_eq!(
        current.body["data"]["visibility"].as_str(),
        Some("role:member")
    );
    let saved = call(
        &owner,
        Method::PUT,
        &format!("{}/{id}", entries(&project)),
        Some(&article("Gated still", "text")),
        &[("if-match", &etag), ("idempotency-key", &key())],
    )
    .await
    .expect("request");
    assert_eq!(
        saved.body["data"]["revision"]["visibility"].as_str(),
        Some("role:member"),
        "{:?}",
        saved.body
    );

    // And asked for, it changes — `public` included, which needs no role.
    let saved = call(
        &owner,
        Method::PUT,
        &format!("{}/{id}?visibility=public", entries(&project)),
        Some(&article("Open now", "text")),
        &[
            ("if-match", &saved.etag().expect("an ETag")),
            ("idempotency-key", &key()),
        ],
    )
    .await
    .expect("request");
    assert_eq!(
        saved.body["data"]["revision"]["visibility"].as_str(),
        Some("public"),
        "{:?}",
        saved.body
    );

    // `create_article` gives no gate: `public` is the default.
    let plain = create_article(&owner, &project, "Plain").await;
    let read = call(
        &owner,
        Method::GET,
        &format!("{}/{plain}", entries(&project)),
        None,
        &[],
    )
    .await
    .expect("request");
    assert_eq!(read.body["data"]["visibility"].as_str(), Some("public"));
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one journey, told in order: request, consume, single use, supersede, no open role"
)]
async fn magic_links_sign_in_existing_visitors_and_create_new_ones() {
    // The production shape first: without COLOPHON_DEV_MAIL a request answers only "sent",
    // carrying no link anyone could read.
    let _plain = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let staff = format!("/api/v1/projects/{project}/audience");
    let made = call(
        &owner,
        Method::POST,
        &format!("{staff}/roles"),
        Some(&json!({"name": "member", "self_assign": true})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let email = format!("linked-{}@example.test", unique_slug());
    let production = audience_post(
        &project,
        "magic-link",
        &json!({"email": email, "return_to": "http://site.test/magic"}),
    )
    .await;
    assert_eq!(production.status(), StatusCode::ACCEPTED);
    let body: Value = production.json().await.expect("json");
    assert_eq!(body["data"]["sent"].as_bool(), Some(true));
    assert!(
        body["data"]["dev_link"].is_null(),
        "no link without COLOPHON_DEV_MAIL: {body}"
    );

    // The development stack has no mail server: with the loopback-only COLOPHON_DEV_MAIL the
    // request also hands the link back, so the whole flow can run end to end.
    let _mail = Server::start_on(PORT + 50, &[("COLOPHON_DEV_MAIL", "1")]).await;
    let dev = format!("http://127.0.0.1:{}", PORT + 50);
    let request = |email: &str| {
        let dev = dev.clone();
        let project = project.clone();
        let email = email.to_owned();
        async move {
            let response = audience_post_at(
                &dev,
                &project,
                "magic-link",
                &json!({"email": email, "return_to": "http://site.test/magic"}),
            )
            .await;
            assert_eq!(response.status(), StatusCode::ACCEPTED);
            let body: Value = response.json().await.expect("json");
            (
                body["data"]["dev_link"]
                    .as_str()
                    .expect("a development link")
                    .to_owned(),
                body,
            )
        }
    };

    // `return_to` is checked: only an absolute http(s) URL, ever.
    for bad in ["/relative/path", "ftp://x.test", "javascript:alert(1)"] {
        let refused = audience_post_at(
            &dev,
            &project,
            "magic-link",
            &json!({"email": "a@b.c", "return_to": bad}),
        )
        .await;
        assert_eq!(refused.status(), StatusCode::UNPROCESSABLE_ENTITY, "{bad}");
        let body: Value = refused.json().await.expect("json");
        assert_eq!(
            body["error"]["details"]["reason"].as_str(),
            Some("invalid_return_to"),
            "{bad}: {body}"
        );
    }

    // A gated read to prove the session the link makes: a member-only entry, published.
    let gated = create_gated(&owner, &project, "For members", "role:member").await;
    release(
        &owner,
        &project,
        &json!({"title": "Gates", "entries": [gated]}),
    )
    .await;

    // An unknown email: the link is the signup — consume creates the account in the
    // project's one open role and answers like a sign-in.
    let (link, _) = request(&email).await;
    let token = dev_token(&link).to_owned();
    let consumed =
        audience_post_at(&dev, &project, &format!("magic-link/{token}"), &json!({})).await;
    assert_eq!(consumed.status(), StatusCode::OK);
    let (cookies, csrf) = audience_cookie(&consumed);
    let body: Value = consumed.json().await.expect("json");
    let session = body["data"]["session"]
        .as_str()
        .expect("a session")
        .to_owned();
    let user = body["data"]["user"]["id"].as_str().expect("a user");
    assert_eq!(body["data"]["user"]["role"].as_str(), Some("member"));
    let (status, me_body) = me(&project, &[("authorization", &format!("Bearer {session}"))]).await;
    assert_eq!(
        (status, me_body["data"]["email"].as_str()),
        (StatusCode::OK, Some(email.as_str()))
    );

    // The session reads the gate it was made for — bypassing the shared cache like any
    // member read.
    let member = delivered(&project, &[("authorization", &format!("Bearer {session}"))]).await;
    assert_eq!(member.titles, ["For members"]);
    assert_eq!(
        (member.cache.as_str(), member.cache_control.as_str()),
        ("bypass", MEMBER_CACHE)
    );

    // And a dead link is dead: the same token answers invalid, never a second session.
    let again = audience_post_at(&dev, &project, &format!("magic-link/{token}"), &json!({})).await;
    assert_eq!(again.status(), StatusCode::FORBIDDEN);
    let body: Value = again.json().await.expect("json");
    assert_eq!(
        body["error"]["details"]["reason"].as_str(),
        Some("magic_link_invalid"),
        "{body}"
    );

    // Requesting again retires the old link: the first token is invalid, the newest signs
    // the same account back in (an existing email takes the sign-in branch, never a
    // duplicate user).
    let (stale, _) = request(&email).await;
    let (live, _) = request(&email).await;
    let superseded = audience_post_at(
        &dev,
        &project,
        &format!("magic-link/{}", dev_token(&stale)),
        &json!({}),
    )
    .await;
    assert_eq!(
        superseded.status(),
        StatusCode::FORBIDDEN,
        "the older link died"
    );
    let consumed = audience_post_at(
        &dev,
        &project,
        &format!("magic-link/{}", dev_token(&live)),
        &json!({}),
    )
    .await;
    assert_eq!(consumed.status(), StatusCode::OK);
    let body: Value = consumed.json().await.expect("json");
    assert_eq!(
        body["data"]["user"]["id"].as_str(),
        Some(user),
        "the account the first link made, not a second"
    );

    // A project whose only role is staff-assigned answers the request exactly the same —
    // and refuses at consume, where the account would be created.
    let closed = common::project(&owner).await;
    let made = call(
        &owner,
        Method::POST,
        &format!("/api/v1/projects/{closed}/audience/roles"),
        Some(&json!({"name": "vip", "self_assign": false})),
        &[],
    )
    .await
    .expect("request");
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let nobody = format!("nobody-{}@example.test", unique_slug());
    let asked = audience_post_at(
        &dev,
        &closed,
        "magic-link",
        &json!({"email": nobody, "return_to": "http://site.test/magic"}),
    )
    .await;
    assert_eq!(asked.status(), StatusCode::ACCEPTED);
    let body: Value = asked.json().await.expect("json");
    let token = dev_token(body["data"]["dev_link"].as_str().expect("a link")).to_owned();
    let refused = audience_post_at(&dev, &closed, &format!("magic-link/{token}"), &json!({})).await;
    assert_eq!(refused.status(), StatusCode::FORBIDDEN);
    let body: Value = refused.json().await.expect("json");
    assert_eq!(
        body["error"]["details"]["reason"].as_str(),
        Some("no_open_role"),
        "{body}"
    );

    // The cookie pair the consume answer set signs out the same way a signup's does.
    let signout = app()
        .post(format!("{dev}/audience/v1/{project}/signout"))
        .header(header::COOKIE, &cookies)
        .header("x-csrf-token", csrf)
        .send()
        .await
        .expect("request");
    assert_eq!(signout.status(), StatusCode::NO_CONTENT);
}
