//! `colophon_audience` against real PostgreSQL (D-235, migration 0089): the transaction the
//! public audience routes open ([`audience::project_tx`]) runs as a role that can reach the
//! audience's own tables and nothing else. A visitor names the project by its site's URL, so
//! the refusal must come from the database for the visitor's own project and any other one
//! alike. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, audience,
    entries::{self, NewRevision},
    projects, scope,
};
use common::TestDb;
use uuid::Uuid;

/// A project with one entry in it, made by its owner; returns the project.
async fn project_with_an_entry(db: &TestDb, owner: Uuid, slug: &str) -> Uuid {
    let project = projects::create(&db.app, owner, slug, slug)
        .await
        .expect("created")
        .id;
    let mut tx = scope::begin_in_project(&db.app, owner, project, "production")
        .await
        .expect("begin")
        .expect("a member");
    entries::create(
        &mut tx,
        Uuid::now_v7(),
        "articles",
        NewRevision {
            id: Uuid::now_v7(),
            content: r#"{"title":"Members only"}"#,
            content_hash: &[3; 32],
            title: "Members only",
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: Some("public"),
        },
        None,
    )
    .await
    .expect("created");
    tx.commit().await.expect("commit");
    project
}

/// The tables a visitor's transaction must never reach: the project's content, its people, its
/// tokens, its environments and its content types.
const FORBIDDEN: [&str; 8] = [
    "entries",
    "revisions",
    "assets",
    "api_tokens",
    "memberships",
    "environments",
    "collections",
    "content_type_versions",
];

/// Runs `statement` (with the project as `$1`) in a fresh audience transaction entered for
/// `entered`; the SQLSTATE it failed with, or `None` if it ran.
async fn refusal(pool: &PgPool, entered: Uuid, statement: &str, project: Uuid) -> Option<String> {
    let mut tx = audience::project_tx(pool, entered).await.expect("begin");
    let outcome = sqlx::query(sqlx::AssertSqlSafe(statement.to_owned()))
        .bind(project)
        .execute(&mut *tx)
        .await;
    match outcome {
        Ok(_) => None,
        Err(error) => Some(
            error
                .as_database_error()
                .and_then(|e| e.code().map(std::borrow::Cow::into_owned))
                .unwrap_or_else(|| error.to_string()),
        ),
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_visitor_transaction_is_refused_every_data_table_in_any_project() {
    let db = TestDb::new().await;
    let owner = db.administrator("owner").await;
    let site = project_with_an_entry(&db, owner, "site").await;
    let other = project_with_an_entry(&db, owner, "other").await;

    // The transaction is the role, not colophon_api with a project set.
    let mut tx = audience::project_tx(&db.app, site).await.expect("begin");
    let (user, project): (String, String) =
        sqlx::query_as("SELECT current_user::text, current_setting('app.project_id')")
            .fetch_one(&mut *tx)
            .await
            .expect("who");
    assert_eq!(
        (user.as_str(), project),
        ("colophon_audience", site.to_string())
    );
    drop(tx);

    let mut admitted = Vec::new();
    for table in FORBIDDEN {
        for (whose, project) in [("its own", site), ("another", other)] {
            for statement in [
                format!("SELECT count(*) FROM {table} WHERE project_id = $1"),
                format!("INSERT INTO {table} (project_id) VALUES ($1)"),
                format!("UPDATE {table} SET project_id = project_id WHERE project_id = $1"),
            ] {
                let code = refusal(&db.app, site, &statement, project).await;
                // 42501, insufficient_privilege: refused before any row is looked at, never an
                // empty answer that row-level security happened to give.
                if code.as_deref() != Some("42501") {
                    admitted.push(format!("{statement} ({whose} project): {code:?}"));
                }
            }
        }
    }
    assert_eq!(
        admitted,
        Vec::<String>::new(),
        "every statement refused with 42501"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn colophon_audience_holds_exactly_the_privileges_the_audience_flows_use() {
    let db = TestDb::new().await;
    let owner_pool = db.owner().await;
    // Every relation outside the system schemas, every table privilege, whole-table or on any
    // column (PUBLIC's included: has_*_privilege counts what the role holds through it).
    let held: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname || ' ' || p.privilege
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         CROSS JOIN unnest(ARRAY['SELECT', 'INSERT', 'UPDATE', 'DELETE', 'TRUNCATE', 'REFERENCES', 'TRIGGER']) AS p (privilege)
         WHERE n.nspname NOT IN ('pg_catalog', 'information_schema') AND n.nspname NOT LIKE 'pg\\_%'
           AND c.relkind IN ('r', 'p', 'v', 'm', 'f', 'S')
           AND CASE WHEN p.privilege IN ('SELECT', 'INSERT', 'UPDATE', 'REFERENCES') AND c.relkind <> 'S'
                    THEN has_any_column_privilege('colophon_audience', c.oid, p.privilege)
                    WHEN c.relkind = 'S'
                    THEN p.privilege IN ('SELECT', 'UPDATE')
                         AND has_sequence_privilege('colophon_audience', c.oid, p.privilege)
                    ELSE has_table_privilege('colophon_audience', c.oid, p.privilege) END
         ORDER BY 1",
    )
    .fetch_all(&owner_pool)
    .await
    .expect("privileges");
    let expected = [
        "audience_magic_links INSERT",
        "audience_magic_links SELECT",
        "audience_magic_links UPDATE",
        "audience_origins SELECT",
        "audit_events INSERT",
        "change_hints INSERT",
        "sign_in_throttle INSERT",
        "sign_in_throttle SELECT",
        "sign_in_throttle UPDATE",
        "site_roles SELECT",
        "site_sessions DELETE",
        "site_sessions INSERT",
        "site_sessions SELECT",
        "site_sessions UPDATE",
        "site_users INSERT",
        "site_users SELECT",
        "site_users UPDATE",
    ];
    assert_eq!(
        held, expected,
        "colophon_audience reaches only the audience tables"
    );

    // The columns it may write, exactly: nothing a visitor could use to move a row elsewhere
    // or rewrite what an account is.
    let writable: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname || '.' || a.attname || ' UPDATE'
         FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace AND n.nspname = 'public'
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attnum > 0 AND NOT a.attisdropped
         WHERE c.relkind = 'r' AND has_column_privilege('colophon_audience', c.oid, a.attnum, 'UPDATE')
         ORDER BY 1",
    )
    .fetch_all(&owner_pool)
    .await
    .expect("columns");
    assert_eq!(
        writable,
        [
            "audience_magic_links.used_at UPDATE",
            "sign_in_throttle.failures UPDATE",
            "sign_in_throttle.locked_until UPDATE",
            "sign_in_throttle.trusted UPDATE",
            "sign_in_throttle.updated_at UPDATE",
            "site_sessions.revoked_at UPDATE",
            "site_users.failed_sign_ins UPDATE",
            "site_users.last_sign_in_at UPDATE",
            "site_users.locked_until UPDATE",
        ],
        "the columns colophon_audience may update"
    );

    // The role itself: no login, no inheritance into colophon_app, only the switch.
    let (login, bypass, set, inherit): (bool, bool, bool, bool) = sqlx::query_as(
        "SELECT r.rolcanlogin, r.rolbypassrls, m.set_option, m.inherit_option
         FROM pg_roles r
         JOIN pg_auth_members m ON m.roleid = r.oid
         JOIN pg_roles app ON app.oid = m.member AND app.rolname = 'colophon_app'
         WHERE r.rolname = 'colophon_audience'",
    )
    .fetch_one(&owner_pool)
    .await
    .expect("role");
    assert_eq!((login, bypass, set, inherit), (false, false, true, false));
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_visitor_transaction_writes_audit_rows_only_as_the_audience_actor() {
    let db = TestDb::new().await;
    let owner = db.administrator("owner").await;
    let site = project_with_an_entry(&db, owner, "site").await;
    let other = project_with_an_entry(&db, owner, "other").await;
    audience::record_auth(&db.app, site, "audience.user_signed_in", "someone", "{}")
        .await
        .expect("the audience actor's own event");
    let insert = "INSERT INTO audit_events (project_id, id, actor_id, action, subject, detail)
                  VALUES ($1, gen_random_uuid(), $2, $3, 's', '{}')";
    for (project, actor, action) in [
        (site, owner, "audience.user_signed_in"),
        (site, audience::AUDIENCE_ACTOR, "token.created"),
        (other, audience::AUDIENCE_ACTOR, "audience.user_signed_in"),
    ] {
        let mut tx = audience::project_tx(&db.app, site).await.expect("begin");
        let refused = sqlx::query(insert)
            .bind(project)
            .bind(actor)
            .bind(action)
            .execute(&mut *tx)
            .await
            .expect_err("refused");
        assert_eq!(
            refused
                .as_database_error()
                .and_then(sqlx::error::DatabaseError::code)
                .as_deref(),
            Some("42501"),
            "{project} {actor} {action}: row-level security refuses it"
        );
    }
    db.drop().await;
}
