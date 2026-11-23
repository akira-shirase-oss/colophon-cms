//! Self-serve projects (D-235), against the real binary: on an installation with open
//! registration any signed-in account creates projects for itself, up to
//! `COLOPHON_PROJECTS_PER_ACCOUNT`; administrators are bound by no cap; an invite-only or closed
//! installation leaves project creation to administrators. Needs the stack and
//! `scripts/migrate.sh` (dev-owner is the administrator).
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use common::{Server, Session, call, get, owner_database, sign_in, unique_slug};
use reqwest::{Method, StatusCode};
use serde_json::json;

/// A fresh account of its own, signed in (signed up on first use).
async fn account() -> Session {
    sign_in(&format!("ss-{}", unique_slug())).await
}

async fn create(session: &Session) -> common::Reply {
    call(
        session,
        Method::POST,
        "/api/v1/projects",
        Some(&json!({"slug": unique_slug(), "name": "Self-serve project"})),
        &[],
    )
    .await
    .expect("request")
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn with_open_registration_any_account_creates_a_project_and_owns_it() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let name = format!("ss-{}", unique_slug());
    let someone = sign_in(&name).await;
    let created = create(&someone).await;
    assert_eq!(created.status, StatusCode::CREATED, "{}", created.body);
    assert_eq!(created.body["data"]["role"], "owner", "{}", created.body);
    let id = created.body["data"]["id"].as_str().expect("an id");
    // The project counts against its creator.
    let counted_against_the_creator: bool = sqlx::query_scalar(
        "SELECT p.owner_account = i.id FROM projects p, identities i
         WHERE p.id = $1::uuid AND i.email = $2",
    )
    .bind(id)
    .bind(format!("{name}@colophon.test"))
    .fetch_one(&owner_database().await)
    .await
    .expect("the project and its creator");
    assert!(
        counted_against_the_creator,
        "owner_account isn't the creator"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn the_project_past_the_cap_is_refused_with_the_limit_and_the_count() {
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_PROJECTS_PER_ACCOUNT", "2"),
    ])
    .await;
    let someone = account().await;
    for n in 1..=2 {
        let created = create(&someone).await;
        assert_eq!(
            created.status,
            StatusCode::CREATED,
            "project {n}: {}",
            created.body
        );
    }
    let third = create(&someone).await;
    assert_eq!(third.status, StatusCode::CONFLICT, "{}", third.body);
    assert_eq!(third.body["error"]["code"], "conflict", "{}", third.body);
    assert_eq!(
        third.body["error"]["details"]["reason"], "project_limit",
        "{}",
        third.body
    );
    assert_eq!(third.body["error"]["details"]["limit"], 2, "{}", third.body);
    assert_eq!(third.body["error"]["details"]["used"], 2, "{}", third.body);
    // Another account has its own allowance.
    let other = create(&account().await).await;
    assert_eq!(other.status, StatusCode::CREATED, "{}", other.body);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn two_creates_racing_for_the_last_place_under_the_cap_let_exactly_one_through() {
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_PROJECTS_PER_ACCOUNT", "1"),
    ])
    .await;
    let someone = account().await;
    for _round in 0..5 {
        let fresh = account().await;
        let (a, b) = tokio::join!(create(&fresh), create(&fresh));
        let statuses = [a.status, b.status];
        assert_eq!(
            statuses
                .iter()
                .filter(|s| **s == StatusCode::CREATED)
                .count(),
            1,
            "exactly one of two racing creates wins: {statuses:?} ({} / {})",
            a.body,
            b.body
        );
        assert!(statuses.contains(&StatusCode::CONFLICT), "{statuses:?}");
    }
    drop(someone);
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh, which bootstraps dev-owner"]
async fn administrators_are_bound_by_no_cap() {
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_PROJECTS_PER_ACCOUNT", "1"),
    ])
    .await;
    let admin = sign_in("dev-owner").await;
    for n in 1..=2 {
        let created = create(&admin).await;
        assert_eq!(
            created.status,
            StatusCode::CREATED,
            "admin's project {n}: {}",
            created.body
        );
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh, which bootstraps dev-owner"]
async fn invite_and_closed_installations_leave_project_creation_to_administrators() {
    for mode in ["invite", "closed"] {
        let server = Server::start_with(&[("COLOPHON_REGISTRATION", mode)]).await;
        // An account that existed before (the dev stack's non-administrator).
        let editor = sign_in("dev-editor").await;
        let refused = create(&editor).await;
        assert_eq!(
            refused.status,
            StatusCode::FORBIDDEN,
            "{mode}: {}",
            refused.body
        );
        assert_eq!(
            refused.body["error"]["details"]["reason"], "administrators_only",
            "{mode}: {}",
            refused.body
        );
        let admin = create(&sign_in("dev-owner").await).await;
        assert_eq!(admin.status, StatusCode::CREATED, "{mode}: {}", admin.body);
        drop(server);
    }
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one catalog snapshot: the column grants, the whole-table grants, the API role, the role itself"
)]
async fn colophon_accounts_holds_exactly_the_grants_it_counts_with() {
    let db = owner_database().await;
    let check = |sql: &'static str| {
        let db = db.clone();
        async move {
            sqlx::query_scalar::<_, bool>(sql)
                .fetch_one(&db)
                .await
                .expect(sql)
        }
    };
    assert!(
        check("SELECT has_column_privilege('colophon_accounts', 'projects', 'id', 'SELECT')").await
    );
    assert!(check("SELECT has_column_privilege('colophon_accounts', 'projects', 'owner_account', 'SELECT')").await);
    for column in ["slug", "name", "created_at"] {
        let can: bool = sqlx::query_scalar(
            "SELECT has_column_privilege('colophon_accounts', 'projects', $1, 'SELECT')",
        )
        .bind(column)
        .fetch_one(&db)
        .await
        .expect("privilege");
        assert!(!can, "colophon_accounts may read projects.{column}");
    }
    // Exactly the grants it counts and reconciles with (0092, 0093) and the sign-up cap's
    // (0095), table and column, and no
    // privilege on any other table by any route (PUBLIC included).
    let granted: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname || ':' || a.privilege_type
         FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace,
              LATERAL aclexplode(c.relacl) a
         WHERE n.nspname = 'public' AND a.grantee = 'colophon_accounts'::regrole
         UNION ALL
         SELECT c.relname || '.' || att.attname || ':' || a.privilege_type
         FROM pg_attribute att JOIN pg_class c ON c.oid = att.attrelid
              JOIN pg_namespace n ON n.oid = c.relnamespace,
              LATERAL aclexplode(att.attacl) a
         WHERE n.nspname = 'public' AND a.grantee = 'colophon_accounts'::regrole
         ORDER BY 1",
    )
    .fetch_all(&db)
    .await
    .expect("grants");
    assert_eq!(
        granted,
        [
            "account_quotas:SELECT",
            "account_quotas:UPDATE",
            "account_upload_reservations:DELETE",
            "account_upload_reservations:INSERT",
            "account_upload_reservations:SELECT",
            "account_usage:INSERT",
            "account_usage:SELECT",
            "account_usage:UPDATE",
            "administrators.identity_id:SELECT",
            "asset_versions.project_id:SELECT",
            "asset_versions.size_bytes:SELECT",
            "assets.project_id:SELECT",
            "assets.size_bytes:SELECT",
            "assets.state:SELECT",
            "entries.project_id:SELECT",
            "projects.id:SELECT",
            "projects.owner_account:SELECT",
            "sign_in_log_budget:INSERT",
            "sign_in_log_budget:SELECT",
            "sign_in_log_budget:UPDATE",
            "signup_throttle:INSERT",
            "signup_throttle:SELECT",
            "signup_throttle:UPDATE",
        ]
    );
    let others: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'v')
           AND c.relname NOT IN ('account_quotas', 'account_upload_reservations', 'account_usage',
                                 'signup_throttle', 'sign_in_log_budget')
           AND (has_table_privilege('colophon_accounts', c.oid, 'SELECT')
             OR has_table_privilege('colophon_accounts', c.oid, 'INSERT')
             OR has_table_privilege('colophon_accounts', c.oid, 'UPDATE')
             OR has_table_privilege('colophon_accounts', c.oid, 'DELETE')
             OR has_table_privilege('colophon_accounts', c.oid, 'TRUNCATE'))
         ORDER BY 1",
    )
    .fetch_all(&db)
    .await
    .expect("tables");
    assert!(
        others.is_empty(),
        "colophon_accounts holds whole-table privileges on {others:?}"
    );
    // The usage tables are written only through the definer functions: the API role holds
    // nothing on them.
    let api: Vec<String> = sqlx::query_scalar(
        "SELECT t FROM unnest(ARRAY['account_quotas', 'account_upload_reservations', 'account_usage', 'signup_throttle', 'sign_in_log_budget']) t
         WHERE has_table_privilege('colophon_api', t, 'SELECT') OR has_table_privilege('colophon_api', t, 'INSERT')
            OR has_table_privilege('colophon_api', t, 'UPDATE') OR has_table_privilege('colophon_api', t, 'DELETE')",
    )
    .fetch_all(&db)
    .await
    .expect("api");
    assert!(api.is_empty(), "colophon_api holds privileges on {api:?}");
    for privilege in ["INSERT", "UPDATE", "DELETE", "TRUNCATE"] {
        let can: bool =
            sqlx::query_scalar("SELECT has_table_privilege('colophon_accounts', 'projects', $1)")
                .bind(privilege)
                .fetch_one(&db)
                .await
                .expect("privilege");
        assert!(!can, "colophon_accounts may {privilege} projects");
    }
    // It belongs to no other role, logs in nowhere, and bypasses nothing.
    assert!(
        check(
            "SELECT NOT r.rolcanlogin AND NOT r.rolsuper AND NOT r.rolbypassrls
                    AND NOT EXISTS (SELECT FROM pg_auth_members m WHERE m.member = r.oid)
             FROM pg_roles r WHERE r.rolname = 'colophon_accounts'"
        )
        .await,
        "colophon_accounts logs in, bypasses row security or inherits another role"
    );
}

/// 0092 lifts row security's FORCE on projects and memberships for its backfill: it must be
/// back on after the migration, or tenant isolation would be off for the owner role for good.
/// (db-nonsuperuser-check.sh checks the same on the non-superuser path.)
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn row_security_is_forced_again_on_projects_and_memberships_after_0092() {
    let forced: Vec<(String, bool)> = sqlx::query_as(
        "SELECT relname::text, relforcerowsecurity FROM pg_class
         WHERE relname IN ('projects', 'memberships') AND relkind = 'r' ORDER BY relname",
    )
    .fetch_all(&owner_database().await)
    .await
    .expect("pg_class");
    assert_eq!(
        forced,
        [
            ("memberships".to_owned(), true),
            ("projects".to_owned(), true)
        ],
        "row security isn't forced on both"
    );
}

/// The account a project counts against never changes (D-235): today the only guard is that no
/// application role may UPDATE projects at all. THREAT: a later feature that grants UPDATE on
/// projects (a rename, say) would let an account move a project onto another account's cap and
/// quotas, or off its own. That feature must fail here first, and add a column-level refusal (a
/// trigger) for `owner_account` before this test may change.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn no_application_role_may_update_a_project_s_owner_account() {
    let db = owner_database().await;
    for role in [
        "colophon_api",
        "colophon_app",
        "colophon_delivery",
        "colophon_worker",
        "colophon_deleter",
        "colophon_retention",
        "colophon_accounts",
    ] {
        let (column, table): (bool, bool) = sqlx::query_as(
            "SELECT has_column_privilege($1, 'projects', 'owner_account', 'UPDATE'),
                    has_table_privilege($1, 'projects', 'UPDATE')",
        )
        .bind(role)
        .fetch_one(&db)
        .await
        .expect("privileges");
        assert!(
            !column && !table,
            "{role} may UPDATE projects (column {column}, table {table})"
        );
    }
}

/// The local stack's object store (the test env's own), for counting what reached it.
fn store() -> object_store::aws::AmazonS3 {
    let env = common::local_env();
    let value = |name: &str| {
        env.iter()
            .find_map(|(key, value)| (key == name).then(|| value.clone()))
            .expect(name)
    };
    object_store::aws::AmazonS3Builder::new()
        .with_endpoint(value("COLOPHON_S3_ENDPOINT"))
        .with_region(value("COLOPHON_S3_REGION"))
        .with_bucket_name(value("COLOPHON_S3_BUCKET"))
        .with_access_key_id(value("COLOPHON_S3_ACCESS_KEY"))
        .with_secret_access_key(value("COLOPHON_S3_SECRET_KEY"))
        .with_allow_http(true)
        .build()
        .expect("object store")
}

/// Objects under a project's quarantine prefix (`quarantine/{project}/{environment}/{name}`).
async fn quarantined(project: &str) -> usize {
    use object_store::ObjectStore as _;
    let store = store();
    let prefix = object_store::path::Path::from(format!("quarantine/{project}"));
    let environments = store
        .list_with_delimiter(Some(&prefix))
        .await
        .expect("listed");
    let mut objects = environments.objects.len();
    for environment in &environments.common_prefixes {
        objects += store
            .list_with_delimiter(Some(environment))
            .await
            .expect("listed")
            .objects
            .len();
    }
    objects
}

async fn upload_bytes(session: &Session, project: &str, bytes: usize) -> common::Reply {
    let response = common::app()
        .post(format!(
            "{}/api/v1/projects/{project}/environments/production/assets",
            common::ORIGIN
        ))
        .header(reqwest::header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(reqwest::header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", common::key())
        .header("x-colophon-file-name", "a.png")
        .header(reqwest::header::CONTENT_TYPE, "image/png")
        .body(vec![b'x'; bytes])
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    common::Reply {
        status,
        headers,
        body: response.json().await.unwrap_or_default(),
    }
}

/// D-235's quotas over HTTP: the entry past the account's limit is 409 `quota_exceeded` with
/// the quota, its limit and the count; an upload past the storage limit is refused before a
/// byte of it reaches the object store (nothing new under the project's quarantine prefix),
/// while one that fits is accepted, verified and held.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_account_past_its_quotas_is_refused_and_an_upload_before_its_bytes_are_stored() {
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_ACCOUNT_MAX_ENTRIES", "2"),
        ("COLOPHON_ACCOUNT_MAX_STORAGE_BYTES", "1000"),
    ])
    .await;
    let alice = account().await;
    let made = create(&alice).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let project = made.body["data"]["id"].as_str().expect("id").to_owned();
    common::create_article(&alice, &project, "one").await;
    common::create_article(&alice, &project, "two").await;
    let third = call(
        &alice,
        Method::POST,
        &common::entries(&project),
        Some(&common::article("three", "text")),
        &[("idempotency-key", &common::key())],
    )
    .await
    .expect("request");
    assert_eq!(third.status, StatusCode::CONFLICT, "{}", third.body);
    let details = &third.body["error"]["details"];
    assert_eq!(third.body["error"]["code"], "conflict", "{}", third.body);
    assert_eq!(details["reason"], "quota_exceeded", "{}", third.body);
    assert_eq!(details["quota"], "entries", "{}", third.body);
    assert_eq!(
        (details["limit"].as_u64(), details["used"].as_u64()),
        (Some(2), Some(2))
    );

    // A fitting upload (a PNG the worker verifies and keeps) is accepted and, once verified,
    // held against the account. Waiting for the verdict keeps the next step independent of
    // the worker: an upload it rejected would hand its bytes back.
    let fits = send_image(
        &alice,
        &format!("/api/v1/projects/{project}/environments/production/assets"),
        PNG,
    )
    .await;
    assert_eq!(fits.status, StatusCode::ACCEPTED, "{}", fits.body);
    let db = owner_database().await;
    let asset = fits.body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        verified(&db, &asset).await,
        0,
        "no reservation outlives the upload"
    );
    let stored: i64 = sqlx::query_scalar(
        "SELECT stored_bytes + pending_bytes FROM account_usage u
         JOIN projects p ON p.owner_account = u.account WHERE p.id = $1::uuid",
    )
    .bind(&project)
    .fetch_one(&db)
    .await
    .expect("usage");
    assert_eq!(
        stored,
        i64::try_from(PNG.len()).expect("small"),
        "the PNG is held"
    );
    // ...and one byte past the limit (held + this > 1000) is refused before its body is read:
    // nothing new under the project's quarantine prefix.
    let before = quarantined(&project).await;
    let over = upload_bytes(&alice, &project, 1000 - PNG.len() + 1).await;
    assert_eq!(over.status, StatusCode::CONFLICT, "{}", over.body);
    assert_eq!(over.body["error"]["details"]["reason"], "quota_exceeded");
    assert_eq!(over.body["error"]["details"]["quota"], "storage_bytes");
    assert_eq!(
        quarantined(&project).await,
        before,
        "the refused upload put nothing in the object store"
    );
}

/// A 1×1 PNG the worker verifies as usable (the asset tests' own).
const PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

/// Sends `bytes` to `path` as an upload (POST, the asset headers).
async fn send_image(session: &Session, path: &str, bytes: &[u8]) -> common::Reply {
    let response = common::app()
        .post(format!("{}{path}", common::ORIGIN))
        .header(reqwest::header::COOKIE, &session.cookie)
        .header("x-csrf-token", &session.csrf)
        .header(reqwest::header::ORIGIN, common::ORIGIN)
        .header("idempotency-key", common::key())
        .header("x-colophon-file-name", "a.png")
        .header(reqwest::header::CONTENT_TYPE, "image/png")
        .body(bytes.to_vec())
        .send()
        .await
        .expect("request");
    let status = response.status();
    let headers = response.headers().clone();
    common::Reply {
        status,
        headers,
        body: response.json().await.unwrap_or_default(),
    }
}

/// Waits (up to 60 s) until every verify job of `asset` has its verdict; then how many intake
/// reservations remain anywhere.
async fn verified(db: &sqlx::PgPool, asset: &str) -> i64 {
    let asset = uuid::Uuid::parse_str(asset).expect("an asset id");
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    loop {
        let open: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM asset_jobs WHERE asset_id = $1
               AND status NOT IN ('succeeded', 'failed', 'superseded')",
        )
        .bind(asset)
        .fetch_one(db)
        .await
        .expect("jobs");
        if open == 0 {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "verification never finished"
        );
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    }
    sqlx::query_scalar("SELECT count(*) FROM account_upload_reservations")
        .fetch_one(db)
        .await
        .expect("reservations")
}

/// A replacement's reservation ends with its verify job (0093's `asset_jobs` trigger), never at its
/// expiry: an asset replaced again and again, each fitting, is never refused by a stale
/// reservation, and no reservation is left once each verdict is in. The limit holds every
/// version even if all are kept, plus one replacement in flight; a stale reservation per
/// replacement would pass it by the fourth.
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn an_asset_replaced_again_and_again_is_never_refused_by_a_stale_reservation() {
    let limit = (PNG.len() * 7).to_string();
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_ACCOUNT_MAX_STORAGE_BYTES", &limit),
    ])
    .await;
    let db = owner_database().await;
    let alice = account().await;
    let made = create(&alice).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let project = made.body["data"]["id"].as_str().expect("id").to_owned();
    let assets = format!("/api/v1/projects/{project}/environments/production/assets");
    let first = send_image(&alice, &assets, PNG).await;
    assert_eq!(first.status, StatusCode::ACCEPTED, "{}", first.body);
    let asset = first.body["data"]["id"].as_str().expect("id").to_owned();
    assert_eq!(
        verified(&db, &asset).await,
        0,
        "the upload's reservation ended"
    );
    for round in 1..=5 {
        let replaced = send_image(&alice, &format!("{assets}/{asset}/replace"), PNG).await;
        assert_eq!(
            replaced.status,
            StatusCode::ACCEPTED,
            "replacement {round}: {}",
            replaced.body
        );
        assert_eq!(
            verified(&db, &asset).await,
            0,
            "replacement {round}: its reservation outlived its verdict"
        );
    }
    // A request that queues no job (here, an asset that doesn't exist: refused after its bytes
    // were reserved and stored) releases its reservation before it answers.
    let unknown = send_image(
        &alice,
        &format!("{assets}/{}/replace", uuid::Uuid::now_v7()),
        PNG,
    )
    .await;
    assert_eq!(unknown.status, StatusCode::NOT_FOUND, "{}", unknown.body);
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM account_upload_reservations")
        .fetch_one(&db)
        .await
        .expect("reservations");
    assert_eq!(left, 0, "a refused replacement left its reservation behind");
}

/// `GET /api/v1/me/usage` as `session`: each meter's (used, limit), limit `None` when unbounded.
async fn usage(session: &Session) -> [(u64, Option<u64>); 4] {
    let (status, body) = get(Some(session), "/api/v1/me/usage").await;
    assert_eq!(status, StatusCode::OK, "{body}");
    ["projects", "entries", "assets", "storage_bytes"].map(|meter| {
        let meter = &body["data"][meter];
        (
            meter["used"].as_u64().expect("used"),
            meter["limit"].as_u64(),
        )
    })
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn each_account_sees_its_own_usage_and_never_another_s() {
    let _server = Server::start_with(&[
        ("COLOPHON_REGISTRATION", "open"),
        ("COLOPHON_PROJECTS_PER_ACCOUNT", "3"),
        ("COLOPHON_ACCOUNT_MAX_ENTRIES", "50"),
    ])
    .await;
    let (alice, bob) = (account().await, account().await);
    assert_eq!(
        usage(&alice).await[0],
        (0, Some(3)),
        "a new account owns nothing"
    );

    let made = create(&alice).await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    let project = made.body["data"]["id"].as_str().expect("id").to_owned();
    common::create_article(&alice, &project, "one").await;
    common::create_article(&alice, &project, "two").await;
    for _ in 0..2 {
        let made = create(&bob).await;
        assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    }

    let [projects, entries, _, _] = usage(&alice).await;
    assert_eq!((projects, entries), ((1, Some(3)), (2, Some(50))));
    let [projects, entries, _, _] = usage(&bob).await;
    assert_eq!(
        (projects, entries),
        ((2, Some(3)), (0, Some(50))),
        "bob's usage carries none of alice's entries, alice's none of his projects"
    );
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh, which bootstraps dev-owner"]
async fn an_administrator_s_usage_is_unbounded_and_a_stranger_gets_401() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "open")]).await;
    let owner = sign_in("dev-owner").await;
    for (meter, (_, limit)) in ["projects", "entries", "assets", "storage_bytes"]
        .iter()
        .zip(usage(&owner).await)
    {
        assert_eq!(limit, None, "{meter} is bounded for an administrator");
    }
    let (status, body) = get(None, "/api/v1/me/usage").await;
    assert_eq!(status, StatusCode::UNAUTHORIZED, "{body}");
}

#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn where_members_create_no_projects_their_project_limit_is_zero() {
    let _server = Server::start_with(&[("COLOPHON_REGISTRATION", "invite")]).await;
    let member = account().await;
    assert_eq!(usage(&member).await[0], (0, Some(0)));
}
