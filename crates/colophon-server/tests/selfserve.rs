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

use common::{Server, Session, call, owner_database, sign_in, unique_slug};
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
async fn colophon_accounts_reads_projects_id_and_owner_and_holds_nothing_else() {
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
    // No privilege on any other table, nor any write on projects.
    let others: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c JOIN pg_namespace n ON n.oid = c.relnamespace
         WHERE n.nspname = 'public' AND c.relkind IN ('r', 'p', 'v')
           AND c.relname NOT IN ('projects', 'account_usage')
           AND (has_table_privilege('colophon_accounts', c.oid, 'SELECT')
             OR has_table_privilege('colophon_accounts', c.oid, 'INSERT')
             OR has_table_privilege('colophon_accounts', c.oid, 'UPDATE')
             OR has_table_privilege('colophon_accounts', c.oid, 'DELETE'))
         ORDER BY 1",
    )
    .fetch_all(&db)
    .await
    .expect("tables");
    assert!(
        others.is_empty(),
        "colophon_accounts holds privileges on {others:?}"
    );
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

/// 0087 lifts row security's FORCE on projects and memberships for its backfill: it must be
/// back on after the migration, or tenant isolation would be off for the owner role for good.
/// (db-nonsuperuser-check.sh checks the same on the non-superuser path.)
#[tokio::test]
#[ignore = "needs the stack and scripts/migrate.sh"]
async fn row_security_is_forced_again_on_projects_and_memberships_after_0087() {
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
