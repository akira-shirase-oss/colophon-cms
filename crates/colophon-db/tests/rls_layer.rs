//! Row-level security as a layer on its own (D-235 review): `colophon_enter_project` is the
//! gate every member route enters through, but the policies must hold without it. A session of
//! the application's role that sets only `app.identity_id` (a foothold before any project is
//! entered, or a future entry path that never sets the project) sees no row of a project it
//! isn't a member of, in any project-keyed table, and can't write one. Needs the local stack.
//!
//! What this layer does NOT stop, by design: a session that also sets `app.project_id` itself.
//! The data tables' policies key on that context, which only the entry functions set after
//! proving membership; a session able to run `set_config` could forge `app.identity_id` just as
//! well, so a membership check in every policy would not stop it either (see the D-235 report).
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::projects;
use common::TestDb;
use sqlx::AssertSqlSafe;

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_identity_alone_sees_and_writes_nothing_of_a_project_it_isn_t_in() {
    let db = TestDb::new().await;
    let (alice, bob) = (db.person("alice").await, db.person("bob").await);
    projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("alice's project");
    let beta = projects::create(&db.app, bob, "beta", "Beta")
        .await
        .expect("bob's project");

    // Every table with a project_id that the application's role may read and that forces row
    // security on it: the whole surface, not a hand-picked list.
    let owner = db.owner().await;
    let tables: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c
         JOIN pg_namespace n ON n.oid = c.relnamespace
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'project_id' AND NOT a.attisdropped
         WHERE n.nspname = 'public' AND c.relkind = 'r' AND c.relrowsecurity
           AND has_table_privilege('colophon_api', c.oid, 'SELECT')
         ORDER BY 1",
    )
    .fetch_all(&owner)
    .await
    .expect("tables");
    assert!(
        tables.len() > 20,
        "too few project-keyed tables: {tables:?}"
    );
    // The positive control: beta's rows exist (as the owner sees them).
    let held: i64 = sqlx::query_scalar("SELECT count(*) FROM memberships WHERE project_id = $1")
        .bind(beta.id)
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(held, 1, "beta has its owner's membership");

    // As colophon_app, in the application's role, with alice's identity and no project.
    let mut connection = db.app.acquire().await.expect("a connection");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, false), set_config('app.project_id', '', false)",
    )
    .bind(alice.to_string())
    .execute(&mut *connection)
    .await
    .expect("context");
    let mut seen = Vec::new();
    for table in &tables {
        // The table name is the catalog's own, quoted by the server.
        let quoted: String = sqlx::query_scalar("SELECT quote_ident($1)")
            .bind(table)
            .fetch_one(&mut *connection)
            .await
            .expect("quote");
        let sql = format!("SELECT count(*) FROM {quoted} WHERE project_id = $1");
        let rows: i64 = sqlx::query_scalar(AssertSqlSafe(sql))
            .bind(beta.id)
            .fetch_one(&mut *connection)
            .await
            .unwrap_or_else(|error| panic!("{table}: {error}"));
        if rows > 0 {
            seen.push(format!("{table}({rows})"));
        }
    }
    // projects itself keys on id, not project_id: checked on its own.
    let project_rows: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE id = $1")
        .bind(beta.id)
        .fetch_one(&mut *connection)
        .await
        .expect("projects");
    if project_rows > 0 {
        seen.push(format!("projects({project_rows})"));
    }
    assert!(
        seen.is_empty(),
        "an identity alone sees another project's rows in {seen:?}"
    );

    // Nor can it write into beta: a project row, a membership, an environment.
    for (what, sql) in [
        (
            "a membership",
            "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'owner')",
        ),
        (
            "an environment",
            "INSERT INTO environments (project_id, id, slug) VALUES ($1, gen_random_uuid(), 'planted')",
        ),
    ] {
        let refused = sqlx::query(sql)
            .bind(beta.id)
            .bind(alice)
            .execute(&mut *connection)
            .await;
        assert!(
            format!("{refused:?}").contains("row-level security"),
            "{what} in beta: {refused:?}"
        );
    }
    drop(connection);
    owner.close().await;
    db.drop().await;
}
