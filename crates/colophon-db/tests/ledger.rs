//! The deletion ledger against real PostgreSQL (migration 0048, D-124): a collection removed
//! outright, a member removed, a role deleted and an entry tombstoned each land their row in
//! the deleting transaction — and vanish with it on rollback — while `colophon_delete_project`
//! writes its row first and takes the project with it. The export carries the interval with
//! its high-water mark; replayed onto a template-cloned "restored backup" it removes exactly
//! what was deleted, idempotently, and the table itself refuses every update and every read
//! through the application's login. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, content_types,
    entries::{self, Deleted, NewRevision},
    ledger,
    members::{self, Accepted, Change},
    projects,
    roles::{self, Removed, Written},
    scope::{self, MemberTransaction, ProjectTransaction},
};
use colophon_domain::policy::Policy;
use common::{TestDb, host};
use sqlx::types::time::OffsetDateTime;
use sqlx::{AssertSqlSafe, postgres::PgPoolOptions};
use uuid::Uuid;

const ARTICLES: &str = "articles";
const SCRATCH: &str = "scratch";
const SCRATCH_DEF: &str = r#"{"name":"Scratch","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false}]}"#;

async fn as_member(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// Alice's project, with Bob joined to it as `role`; `(alice, bob, project)`.
async fn alice_bob_project(db: &TestDb, slug: &str, role: &str) -> (Uuid, Uuid, Uuid) {
    let (alice, bob) = (db.administrator("alice").await, db.person("bob").await);
    let project = projects::create(&db.app, alice, slug, "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    let later = OffsetDateTime::now_utc() + time::Duration::hours(72);
    members::invite(&mut tx, &[7; 32], role, "Bob", later)
        .await
        .expect("invited");
    tx.commit().await.expect("commit");
    assert!(matches!(
        members::accept(&db.app, bob, &[7; 32])
            .await
            .expect("accept"),
        Accepted::Joined { .. }
    ));
    (alice, bob, project)
}

/// `project`'s environment id, read as the owner (the 'production' environment a new project
/// is born with).
async fn environment(owner: &PgPool, project: Uuid) -> Uuid {
    sqlx::query_scalar("SELECT id FROM environments WHERE project_id = $1 AND slug = 'production'")
        .bind(project)
        .fetch_one(owner)
        .await
        .expect("the production environment")
}

/// An `articles` entry in `tx`, one revision; its ids.
async fn entry(tx: &mut ProjectTransaction) -> (Uuid, Uuid) {
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        tx,
        entry,
        ARTICLES,
        NewRevision {
            id: revision,
            content: r#"{"title":"Doomed"}"#,
            content_hash: &[7; 32],
            title: "Doomed",
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
        },
        None,
    )
    .await
    .expect("created");
    (entry, revision)
}

/// The ledger's rows, as `(id, kind, environment_id, detail)`, read as the table owner (the
/// application's login can't see it at all); `detail` parsed back from its JSON text.
async fn rows(owner: &PgPool) -> Vec<(i64, String, Option<Uuid>, serde_json::Value)> {
    let read: Vec<(i64, String, Option<Uuid>, String)> = sqlx::query_as(
        "SELECT id, kind, environment_id, detail::text FROM deletion_ledger ORDER BY id",
    )
    .fetch_all(owner)
    .await
    .expect("ledger rows");
    read.into_iter()
        .map(|(id, kind, env, detail)| {
            (
                id,
                kind,
                env,
                serde_json::from_str(&detail).expect("the ledger's detail is JSON"),
            )
        })
        .collect()
}

/// Deletes `project` the way an operator does (D-167): `colophon_delete_project` isn't the
/// application's to call, so it runs on the owner's login, in the deleting person's context.
async fn delete_project(owner: &PgPool, actor: Uuid, project: Uuid) -> String {
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true), set_config('app.project_id', $2, true)",
    )
    .bind(actor.to_string())
    .bind(project.to_string())
    .execute(&mut *tx)
    .await
    .expect("the deleting context");
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM colophon_delete_project($1)")
        .bind(project)
        .fetch_one(&mut *tx)
        .await
        .expect("colophon_delete_project");
    tx.commit().await.expect("commit");
    outcome
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn every_governed_deletion_lands_its_row_in_the_deleting_transaction() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_bob_project(&db, "alpha", "editor").await;
    let owner = db.owner().await;

    // A member removed: the membership row goes, and its ledger row is in the same commit.
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::remove(&mut tx, bob).await.expect("removed"),
        Change::Done
    );
    tx.commit().await.expect("commit");

    // A role deleted once nobody holds it.
    let mut tx = as_member(&db.app, alice, project).await;
    assert!(matches!(
        roles::create(&mut tx, "night-desk", "Night desk", "", &Policy::default())
            .await
            .expect("create"),
        Written::Done(_)
    ));
    tx.commit().await.expect("commit");
    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        roles::delete(&mut tx, "night-desk", 1)
            .await
            .expect("delete"),
        Removed::Done
    );
    tx.commit().await.expect("commit");

    // An entry tombstoned through colophon_delete_entry.
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, revision) = entry(&mut tx).await;
    tx.commit().await.expect("commit");
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        entries::delete(&mut tx, ARTICLES, entry, revision)
            .await
            .expect("delete"),
        Deleted::Deleted { .. }
    ));
    tx.commit().await.expect("commit");

    // A collection removed outright (it never held an entry, so it may go).
    let mut tx = enter(&db.app, alice, project).await;
    content_types::create(&mut tx, SCRATCH, SCRATCH_DEF, &[9; 32])
        .await
        .expect("collection");
    tx.commit().await.expect("commit");
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        content_types::remove(&mut tx, SCRATCH)
            .await
            .expect("remove"),
        content_types::Removed::Removed
    );
    tx.commit().await.expect("commit");

    let seen = rows(&owner).await;
    assert_eq!(
        seen.iter().map(|row| row.1.as_str()).collect::<Vec<_>>(),
        ["member", "role", "entry", "collection"],
        "one row a deletion, in order"
    );
    let environment = environment(&owner, project).await;
    for (id, kind, env, detail) in &seen {
        match kind.as_str() {
            "member" => {
                assert_eq!(detail["identity"], bob.to_string(), "{id}: {detail}");
                assert_eq!(*env, None);
            }
            "role" => {
                assert_eq!(detail["role"], "night-desk", "{id}: {detail}");
                assert_eq!(*env, None);
            }
            "entry" => {
                assert_eq!(detail["entry"], entry.to_string(), "{id}: {detail}");
                assert_eq!(*env, Some(environment));
            }
            "collection" => {
                assert_eq!(detail["collection"], SCRATCH, "{id}: {detail}");
                assert_eq!(*env, Some(environment));
            }
            other => panic!("an unknown kind was written: {other}"),
        }
    }

    // A project deleted: its row is written first and survives the project, by not being its.
    assert_eq!(delete_project(&owner, alice, project).await, "deleted");
    let (projects_left,): (i64,) = sqlx::query_as("SELECT count(*) FROM projects WHERE id = $1")
        .bind(project)
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(projects_left, 0, "the project is gone outright");
    let seen = rows(&owner).await;
    assert_eq!(seen.len(), 5);
    assert_eq!(seen[4].1, "project");
    assert_eq!(seen[4].3["slug"], "alpha", "{}", seen[4].3);
    assert_eq!(seen[4].2, None, "a project is not environment-scoped");

    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_rolled_back_deletion_leaves_no_ledger_row() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_bob_project(&db, "alpha", "editor").await;
    let owner = db.owner().await;

    let mut tx = as_member(&db.app, alice, project).await;
    assert_eq!(
        members::remove(&mut tx, bob).await.expect("removed"),
        Change::Done
    );
    // The deletion and its ledger row commit together or not at all: the row is never a
    // ghost the restore would chase.
    tx.transaction.rollback().await.expect("rollback");

    let (membership, ledgered): (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM memberships WHERE project_id = $1 AND identity_id = $2),
                (SELECT count(*) FROM deletion_ledger)",
    )
    .bind(project)
    .bind(bob)
    .fetch_one(&owner)
    .await
    .expect("counts");
    assert_eq!((membership, ledgered), (1, 0));

    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one restore drill told in order: clone, delete, export, replay, replay again"
)]
async fn the_export_replayed_removes_what_the_backup_still_holds() {
    let db = TestDb::new().await;
    // Two projects: "doomed" is deleted whole (F14's case); "scarred" keeps its row while a
    // member, a role, an entry and a collection are removed from it.
    let alice = db.administrator("alice").await;
    let carol = db.person("carol").await;
    let doomed = projects::create(&db.app, alice, "doomed", "Doomed")
        .await
        .expect("created")
        .id;
    let scarred = projects::create(&db.app, alice, "scarred", "Scarred")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, scarred).await;
    let later = OffsetDateTime::now_utc() + time::Duration::hours(72);
    members::invite(&mut tx, &[8; 32], "editor", "Carol", later)
        .await
        .expect("invited");
    roles::create(&mut tx, "night-desk", "Night desk", "", &Policy::default())
        .await
        .expect("role");
    tx.commit().await.expect("commit");
    assert!(matches!(
        members::accept(&db.app, carol, &[8; 32])
            .await
            .expect("accept"),
        Accepted::Joined { .. }
    ));
    let owner = db.owner().await;
    let scarred_environment = environment(&owner, scarred).await;
    let mut tx = enter(&db.app, alice, scarred).await;
    let (entry, revision) = entry(&mut tx).await;
    content_types::create(&mut tx, SCRATCH, SCRATCH_DEF, &[9; 32])
        .await
        .expect("collection");
    tx.commit().await.expect("commit");

    // The "backup": a template clone of the database as it is now, before any deletion. No
    // connection may hang on the template's source, so the pools close first.
    db.app.close().await;
    owner.close().await;
    let backup = format!("colophon_test_{}", Uuid::now_v7().simple());
    sqlx::query(AssertSqlSafe(format!(
        "CREATE DATABASE {backup} TEMPLATE {}",
        db.name
    )))
    .execute(&db.admin)
    .await
    .expect("the backup clone");
    let restored = PgPoolOptions::new()
        .max_connections(2)
        .connect(&format!(
            "postgres://colophon:colophon-dev@{}/{backup}",
            host()
        ))
        .await
        .expect("connect to the restored backup");
    let app = db.app_pool(1).await;

    // On the live database: the member, role, entry and collection go, then the whole of
    // "doomed".
    let mut tx = as_member(&app, alice, scarred).await;
    assert_eq!(
        members::remove(&mut tx, carol).await.expect("removed"),
        Change::Done
    );
    tx.commit().await.expect("commit");
    let mut tx = as_member(&app, alice, scarred).await;
    assert_eq!(
        roles::delete(&mut tx, "night-desk", 1)
            .await
            .expect("delete"),
        Removed::Done
    );
    tx.commit().await.expect("commit");
    let mut tx = enter(&app, alice, scarred).await;
    assert!(matches!(
        entries::delete(&mut tx, ARTICLES, entry, revision)
            .await
            .expect("delete"),
        Deleted::Deleted { .. }
    ));
    assert_eq!(
        content_types::remove(&mut tx, SCRATCH)
            .await
            .expect("remove"),
        content_types::Removed::Removed
    );
    tx.commit().await.expect("commit");
    let live_owner = db.owner().await;
    assert_eq!(delete_project(&live_owner, alice, doomed).await, "deleted");
    live_owner.close().await;

    // The export: a header's watermark over the interval's rows, all of them.
    let owner = db.owner().await;
    let mut bytes = Vec::new();
    let exported = ledger::export(&owner, 0, &mut bytes).await.expect("export");
    assert_eq!(exported.rows, 5, "member, role, entry, collection, project");
    let parsed = ledger::unmarshal(&bytes).expect("the export parses back");
    assert_eq!(parsed.high_water, exported.high_water);
    assert_eq!(parsed.rows.len(), 5);
    assert!(parsed.rows.windows(2).all(|pair| pair[0].id < pair[1].id));
    assert_eq!(parsed.rows[2].environment_id, Some(scarred_environment));

    // Replayed on the restored backup: everything the backup still holds is removed.
    let replayed = ledger::replay(&restored, &parsed.rows)
        .await
        .expect("replay");
    assert_eq!(
        (replayed.applied, replayed.skipped, replayed.conflicts.len()),
        (5, 0, 0),
        "{replayed:?}"
    );
    let (doomed_left, memberships, role_rows, collections, tombstoned): (i64, i64, i64, i64, i64) =
        sqlx::query_as(
            "SELECT (SELECT count(*) FROM projects WHERE id = $1),
                (SELECT count(*) FROM memberships WHERE project_id = $2 AND identity_id = $3),
                (SELECT count(*) FROM roles WHERE project_id = $2 AND key = 'night-desk'),
                (SELECT count(*) FROM collections WHERE project_id = $2 AND slug = $4),
                (SELECT count(*) FROM entries
                  WHERE project_id = $2 AND id = $5 AND deleted_at IS NOT NULL)",
        )
        .bind(doomed)
        .bind(scarred)
        .bind(carol)
        .bind(SCRATCH)
        .bind(entry)
        .fetch_one(&restored)
        .await
        .expect("the restored state");
    assert_eq!(
        doomed_left, 0,
        "F14: the deleted project does not resurrect"
    );
    assert_eq!(memberships, 0, "the removed member stays removed");
    assert_eq!(role_rows, 0, "the deleted role stays deleted");
    assert_eq!(collections, 0, "the removed collection stays removed");
    assert_eq!(tombstoned, 1, "the entry is a tombstone again");

    // The restored ledger holds the interval itself, at its own ids — plus the honest row
    // colophon_delete_project wrote for the deletion the replay performed here.
    let (ledgered, high): (i64, i64) =
        sqlx::query_as("SELECT count(*), coalesce(max(id), 0) FROM deletion_ledger")
            .fetch_one(&restored)
            .await
            .expect("restored ledger");
    assert_eq!((ledgered, high), (6, parsed.high_water + 1));

    // Idempotent: replaying the same file changes nothing and reports the skips.
    let again = ledger::replay(&restored, &parsed.rows)
        .await
        .expect("replay again");
    assert_eq!(
        (again.applied, again.skipped, again.conflicts.len()),
        (0, 5, 0),
        "{again:?}"
    );

    sqlx::query(AssertSqlSafe(format!(
        "DROP DATABASE {backup} WITH (FORCE)"
    )))
    .execute(&db.admin)
    .await
    .expect("drop the backup");
    owner.close().await;
    app.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_table_is_append_only_and_invisible_through_the_application_login() {
    let db = TestDb::new().await;
    let (alice, bob, project) = alice_bob_project(&db, "alpha", "editor").await;
    let owner = db.owner().await;
    let mut tx = as_member(&db.app, alice, project).await;
    members::remove(&mut tx, bob).await.expect("removed");
    tx.commit().await.expect("commit");

    // Even the table owner can't change or drop a row; the application's login can't even
    // read it.
    for statement in [
        "UPDATE deletion_ledger SET kind = 'entry'",
        "DELETE FROM deletion_ledger",
        "TRUNCATE deletion_ledger",
    ] {
        let refused = sqlx::query(statement).execute(&owner).await;
        assert!(refused.is_err(), "{statement} must be refused");
    }
    let read = sqlx::query("SELECT id FROM deletion_ledger")
        .fetch_all(&db.app)
        .await;
    assert!(
        read.is_err(),
        "the application login never reads the ledger"
    );
    let write = sqlx::query(
        "INSERT INTO deletion_ledger (kind, project_id, project_slug, detail)
         VALUES ('member', $1, 'x', '{}')",
    )
    .bind(project)
    .execute(&db.app)
    .await;
    assert!(
        write.is_err(),
        "a row outside the transaction's project context is refused"
    );

    owner.close().await;
    db.drop().await;
}

/// Deleting a project removes every row any project-scoped table holds for it (migration
/// 0061): the catalogue, not a hand-kept list, says which tables those are. A project
/// with an audience (a site role, a visitor, their session, a magic link, a listed origin) and
/// a versioned asset is deleted outright, and every table with a `project_id` column — read
/// from `pg_attribute` — then holds nothing for it, the ledger excepted. The function's own
/// text must name every such table, or reach it through an `ON DELETE CASCADE` key, so a
/// table a later migration adds fails here before any project holds a row in it.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn deleting_a_project_leaves_no_row_in_any_project_table() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "doomed", "Doomed")
        .await
        .expect("created")
        .id;
    let environment = environment(&owner, project).await;
    let asset = Uuid::now_v7();
    let visitor = Uuid::now_v7();
    for statement in [
        "INSERT INTO site_roles (project_id, name) VALUES ($1, 'member')",
        "INSERT INTO site_users (project_id, id, email, password_hash, role)
         VALUES ($1, $3, 'visitor@example.test', 'not-a-hash', 'member')",
        "INSERT INTO site_sessions (project_id, id, site_user_id, token_hash, csrf_hash, expires_at)
         VALUES ($1, gen_random_uuid(), $3, '\\x0101010101010101010101010101010101010101010101010101010101010101',
                 '\\x0202020202020202020202020202020202020202020202020202020202020202',
                 now() + interval '1 hour')",
        "INSERT INTO audience_magic_links (project_id, id, email, token_hash, return_to, expires_at)
         VALUES ($1, gen_random_uuid(), 'visitor@example.test',
                 '\\x0303030303030303030303030303030303030303030303030303030303030303',
                 'https://site.test/in',
                 now() + interval '15 minutes')",
        "INSERT INTO audience_origins (project_id, origin) VALUES ($1, 'https://site.test')",
        "INSERT INTO assets (project_id, environment_id, id, file_name, declared_type, size_bytes,
                             quarantine_key, created_by)
         VALUES ($1, $2, $4, 'photo.png', 'image/png', 10, 'quarantine/' || $4::text, $5)",
        "INSERT INTO asset_versions (project_id, environment_id, asset_id, seq, declared_type,
                                     size_bytes, detected_type, sha256, width, height, public_key)
         VALUES ($1, $2, $4, 1, 'image/png', 10, 'image/png',
                 '\\x0404040404040404040404040404040404040404040404040404040404040404', 1, 1,
                 'public/' || $4::text)",
    ] {
        sqlx::query(statement)
            .bind(project)
            .bind(environment)
            .bind(visitor)
            .bind(asset)
            .bind(alice)
            .execute(&owner)
            .await
            .unwrap_or_else(|error| panic!("{statement}: {error}"));
    }

    assert_eq!(delete_project(&owner, alice, project).await, "deleted");

    // Every table with a project_id, from the catalogue: none holds a row for the project.
    let scoped: Vec<String> = sqlx::query_scalar(
        "SELECT c.relname::text FROM pg_class c
         JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'project_id' AND NOT a.attisdropped
         WHERE c.relkind IN ('r', 'p') AND c.relnamespace = 'public'::regnamespace
           AND c.relname <> 'deletion_ledger'
         ORDER BY 1",
    )
    .fetch_all(&owner)
    .await
    .expect("the catalogue");
    assert!(
        scoped.len() > 40,
        "the catalogue lists the project tables: {scoped:?}"
    );
    for table in &scoped {
        let left: i64 = sqlx::query_scalar(AssertSqlSafe(format!(
            "SELECT count(*) FROM public.{table} WHERE project_id = $1"
        )))
        .bind(project)
        .fetch_one(&owner)
        .await
        .expect("count");
        assert_eq!(left, 0, "{table} still holds rows for the deleted project");
    }

    // And the function names every one of them, or reaches it by a cascading key.
    let unnamed: Vec<String> = sqlx::query_scalar(
        "WITH scoped AS (
             SELECT c.oid, c.relname::text AS name FROM pg_class c
             JOIN pg_attribute a ON a.attrelid = c.oid AND a.attname = 'project_id'
                                AND NOT a.attisdropped
             WHERE c.relkind IN ('r', 'p') AND c.relnamespace = 'public'::regnamespace
               AND c.relname <> 'deletion_ledger'),
         body AS (SELECT pg_get_functiondef('colophon_delete_project(uuid)'::regprocedure) AS def)
         SELECT s.name FROM scoped s, body b
         WHERE position('DELETE FROM public.' || s.name || ' ' IN b.def) = 0
           AND NOT EXISTS (
               SELECT 1 FROM pg_constraint k JOIN pg_class r ON r.oid = k.confrelid
               WHERE k.conrelid = s.oid AND k.contype = 'f' AND k.confdeltype = 'c'
                 AND position('DELETE FROM public.' || r.relname::text || ' ' IN b.def) > 0)
         ORDER BY 1",
    )
    .fetch_all(&owner)
    .await
    .expect("the function's text");
    assert!(
        unnamed.is_empty(),
        "colophon_delete_project neither deletes nor cascades to: {unnamed:?}"
    );

    owner.close().await;
    db.drop().await;
}

/// The application can't delete a project (D-167): `colophon_delete_project` checks only the
/// context settings, which any session can set (D-155's own argument), and no route calls it —
/// so `colophon_api` may not execute it, even as an owner in the project's context.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_application_login_cannot_delete_a_project() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "kept", "Kept")
        .await
        .expect("created")
        .id;
    let mut tx = as_member(&db.app, alice, project).await;
    let refused =
        sqlx::query_scalar::<_, String>("SELECT outcome FROM colophon_delete_project($1)")
            .bind(project)
            .fetch_one(&mut *tx.transaction)
            .await
            .expect_err("the application executed colophon_delete_project");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("42501"),
        "{refused}"
    );
    drop(tx);
    let owner = db.owner().await;
    let left: i64 = sqlx::query_scalar("SELECT count(*) FROM projects WHERE id = $1")
        .bind(project)
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(left, 1);
    owner.close().await;
    db.drop().await;
}
