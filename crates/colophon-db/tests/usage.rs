//! Per-account quotas (D-235; migration 0093): the usage triggers count every write and refuse
//! one past a limit, concurrent writers that each fit alone can't both pass, multi-account
//! statements don't deadlock, deleting a project returns exactly what it held, upload intake
//! reserves bytes before any reach the store, and reconcile finds drift. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    DbError, PgPool, Quota, assets,
    entries::{self, NewRevision},
    projects,
    scope::{self, ProjectTransaction},
    usage::{self, Limits},
};
use common::TestDb;
use time::OffsetDateTime;
use uuid::Uuid;

const UNBOUNDED: Limits = Limits {
    entries: 0,
    assets: 0,
    storage_bytes: 0,
};

fn document() -> NewRevision<'static> {
    NewRevision {
        id: Uuid::now_v7(),
        content: r#"{"title":"Hi"}"#,
        content_hash: &[1; 32],
        title: "Hi",
        slug: None,
        schema_version: 1,
        references: &[],
        assets: &[],
        unique: &[],
        visibility: None,
    }
}

async fn enter(db: &TestDb, who: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(&db.app, who, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// Writes one article in its own transaction.
async fn write(db: &TestDb, who: Uuid, project: Uuid) -> Result<(), DbError> {
    let mut tx = enter(db, who, project).await;
    entries::create(&mut tx, Uuid::now_v7(), "articles", document(), None).await?;
    tx.commit().await
}

/// Records one quarantined upload of `bytes` in its own transaction.
async fn upload(db: &TestDb, who: Uuid, project: Uuid, bytes: i64) -> Result<(), DbError> {
    let mut tx = enter(db, who, project).await;
    let asset = Uuid::now_v7();
    assets::insert(
        &mut tx,
        asset,
        "a.png",
        "image/png",
        bytes,
        &format!("quarantine/{asset}"),
        who,
        Uuid::now_v7(),
        &format!("verify:{asset}"),
        OffsetDateTime::now_utc() + Duration::from_secs(3600),
    )
    .await?;
    tx.commit().await
}

fn refused(result: Result<(), DbError>, quota: Quota) -> (u64, u64) {
    match result {
        Err(DbError::QuotaExceeded {
            quota: found,
            limit,
            used,
        }) if found == quota => (limit, used),
        other => panic!("expected a {quota} refusal, got {other:?}"),
    }
}

async fn project(db: &TestDb, who: Uuid, slug: &str) -> Uuid {
    projects::create(&db.app, who, slug, slug)
        .await
        .expect("project")
        .id
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn entries_and_assets_are_counted_and_refused_past_the_limit_and_administrators_are_not() {
    let db = TestDb::new().await;
    usage::set_limits(
        &db.app,
        Limits {
            entries: 3,
            assets: 2,
            storage_bytes: 0,
        },
    )
    .await
    .expect("limits");
    let alice = db.person("alice").await;
    let (one, two) = (
        project(&db, alice, "one").await,
        project(&db, alice, "two").await,
    );
    // The limit is the account's, across its projects.
    write(&db, alice, one).await.expect("1");
    write(&db, alice, two).await.expect("2");
    write(&db, alice, one).await.expect("3");
    assert_eq!(
        refused(write(&db, alice, two).await, Quota::Entries),
        (3, 3)
    );
    upload(&db, alice, one, 10).await.expect("asset 1");
    upload(&db, alice, two, 10).await.expect("asset 2");
    assert_eq!(
        refused(upload(&db, alice, one, 10).await, Quota::Assets),
        (2, 2)
    );
    let held = usage::of_identity(&db.app, alice).await.expect("usage");
    assert_eq!(
        (held.entries, held.assets, held.pending_bytes),
        (3, 2, 20),
        "the refused writes left nothing behind"
    );

    // A noisy tenant at its limits leaves another account's room alone.
    let bob = db.person("bob").await;
    let bobs = project(&db, bob, "bobs").await;
    write(&db, bob, bobs).await.expect("bob's first");
    // Administrators are counted, never refused.
    let root = db.administrator("root").await;
    let roots = project(&db, root, "roots").await;
    for _ in 0..5 {
        write(&db, root, roots)
            .await
            .expect("an administrator's entry");
    }
    let held = usage::of_identity(&db.app, root).await.expect("usage");
    assert!(held.exempt);
    assert_eq!(held.entries, 5);
    assert_eq!(
        usage::reconcile(&db.owner().await, false)
            .await
            .expect("reconcile"),
        Vec::new(),
        "no drift"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_writes_that_each_fit_alone_cannot_both_pass() {
    let db = TestDb::new().await;
    usage::set_limits(
        &db.app,
        Limits {
            entries: 2,
            ..UNBOUNDED
        },
    )
    .await
    .expect("limits");
    let alice = db.person("alice").await;
    let (one, two) = (
        project(&db, alice, "one").await,
        project(&db, alice, "two").await,
    );
    write(&db, alice, one).await.expect("1 of 2");
    // The first holds the account's usage row until it commits; the second waits on it, then
    // sees the committed total (its own UPDATE's RETURNING) and is refused.
    // Two connections, or the "concurrent" transactions would queue for the one test pool
    // and never meet at the lock.
    let pool = db.app_pool(2).await;
    let mut first = scope::begin_in_project(&pool, alice, one, "production")
        .await
        .expect("begin")
        .expect("a member");
    entries::create(&mut first, Uuid::now_v7(), "articles", document(), None)
        .await
        .expect("fits alone");
    let second = {
        let db_app = pool.clone();
        tokio::spawn(async move {
            let mut tx = scope::begin_in_project(&db_app, alice, two, "production")
                .await
                .expect("begin")
                .expect("a member");
            entries::create(&mut tx, Uuid::now_v7(), "articles", document(), None).await?;
            tx.commit().await
        })
    };
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        !second.is_finished(),
        "the second waits on the first's lock"
    );
    first.commit().await.expect("first commits");
    assert_eq!(
        refused(second.await.expect("joined"), Quota::Entries),
        (2, 2)
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn statements_touching_several_accounts_lock_them_in_one_order() {
    let db = TestDb::new().await;
    let (alice, bob) = (db.person("alice").await, db.person("bob").await);
    let (a, b) = (project(&db, alice, "a").await, project(&db, bob, "b").await);
    for _ in 0..40 {
        write(&db, alice, a).await.expect("alice's");
        write(&db, bob, b).await.expect("bob's");
    }
    // Twenty rounds of two concurrent statements, each deleting rows of both accounts, listed
    // in opposite orders: the triggers take the usage rows in account order, so none deadlocks.
    let owner = db.owner().await;
    for _ in 0..20 {
        let delete = |owner: PgPool, first: Uuid, second: Uuid| async move {
            let mut tx = owner.begin().await.expect("begin");
            sqlx::query(
                "WITH picked AS (
                     (SELECT id FROM entries WHERE project_id = $1 ORDER BY id LIMIT 1)
                     UNION ALL
                     (SELECT id FROM entries WHERE project_id = $2 ORDER BY id DESC LIMIT 1)),
                 drafts AS (DELETE FROM revisions WHERE entry_id IN (SELECT id FROM picked))
                 DELETE FROM entries WHERE id IN (SELECT id FROM picked)",
            )
            .bind(first)
            .bind(second)
            .execute(&mut *tx)
            .await?;
            tokio::time::sleep(Duration::from_millis(20)).await;
            tx.commit().await
        };
        let (x, y) = tokio::join!(delete(owner.clone(), a, b), delete(owner.clone(), b, a));
        x.expect("no deadlock");
        y.expect("no deadlock");
    }
    assert_eq!(
        usage::reconcile(&owner, false).await.expect("reconcile"),
        Vec::new(),
        "no drift"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn deleting_a_project_returns_exactly_what_it_held() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let (kept, gone) = (
        project(&db, alice, "kept").await,
        project(&db, alice, "gone").await,
    );
    write(&db, alice, kept).await.expect("kept entry");
    upload(&db, alice, kept, 100).await.expect("kept asset");
    let before = usage::of_identity(&db.app, alice).await.expect("usage");
    for _ in 0..3 {
        write(&db, alice, gone).await.expect("gone entry");
        upload(&db, alice, gone, 1_000).await.expect("gone asset");
    }
    // As a live deletion runs it (the deleter's path, D-171): in the project's own context.
    let owner = db.owner().await;
    let mut tx = owner.begin().await.expect("begin");
    sqlx::query(
        "SELECT set_config('app.identity_id', $1, true), set_config('app.project_id', $2, true)",
    )
    .bind(alice.to_string())
    .bind(gone.to_string())
    .execute(&mut *tx)
    .await
    .expect("context");
    let outcome: String = sqlx::query_scalar("SELECT outcome FROM colophon_delete_project($1)")
        .bind(gone)
        .fetch_one(&mut *tx)
        .await
        .expect("deleted");
    assert_eq!(outcome, "deleted");
    tx.commit().await.expect("commit");
    let after = usage::of_identity(&db.app, alice).await.expect("usage");
    assert_eq!(
        after, before,
        "the deleted project's share, no more, no less"
    );
    assert_eq!(
        usage::reconcile(&db.owner().await, false)
            .await
            .expect("reconcile"),
        Vec::new(),
        "no drift"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn uploads_reserve_their_bytes_before_any_reach_the_store() {
    let db = TestDb::new().await;
    usage::set_limits(
        &db.app,
        Limits {
            storage_bytes: 1_000,
            ..UNBOUNDED
        },
    )
    .await
    .expect("limits");
    let alice = db.person("alice").await;
    let one = project(&db, alice, "one").await;
    upload(&db, alice, one, 300).await.expect("300 held");
    // 300 held + 600 reserved fits; another 600 doesn't, while the first is in flight.
    let first = Uuid::now_v7();
    let mut tx = enter(&db, alice, one).await;
    usage::reserve(&mut tx, first, 600).await.expect("fits");
    tx.commit().await.expect("commit");
    let mut tx = enter(&db, alice, one).await;
    assert_eq!(
        refused(
            usage::reserve(&mut tx, Uuid::now_v7(), 600).await,
            Quota::StorageBytes
        ),
        (1_000, 900)
    );
    drop(tx);
    // Two reservations racing for the last 100 bytes: exactly one passes.
    let racing = |pool: PgPool| async move {
        let mut tx = scope::begin_in_project(&pool, alice, one, "production")
            .await
            .expect("begin")
            .expect("a member");
        usage::reserve(&mut tx, Uuid::now_v7(), 60).await?;
        tokio::time::sleep(Duration::from_millis(100)).await;
        tx.commit().await
    };
    // Two connections, so the two really overlap (the test pool has one).
    let pool = db.app_pool(2).await;
    let (x, y) = tokio::join!(racing(pool.clone()), racing(pool.clone()));
    assert_eq!(
        usize::from(x.is_ok()) + usize::from(y.is_ok()),
        1,
        "{x:?} {y:?}"
    );
    // Recording the upload releases its reservation: its bytes now count as quarantined.
    let mut tx = enter(&db, alice, one).await;
    usage::release(&mut tx, first).await.expect("released");
    tx.commit().await.expect("commit");
    upload(&db, alice, one, 600).await.expect("recorded");
    let held = usage::of_identity(&db.app, alice).await.expect("usage");
    assert_eq!(held.pending_bytes, 900);
    // Another account's project can't release (or see) alice's reservations: a release names
    // a reservation of the entered project's account only.
    let bob = db.person("bob").await;
    let bobs = project(&db, bob, "bobs").await;
    let mut tx = enter(&db, alice, one).await;
    let held_id = Uuid::now_v7();
    usage::reserve(&mut tx, held_id, 40)
        .await
        .expect("the last 40");
    tx.commit().await.expect("commit");
    let mut tx = enter(&db, bob, bobs).await;
    usage::release(&mut tx, held_id).await.expect("a no-op");
    tx.commit().await.expect("commit");
    let owner = db.owner().await;
    let still: i64 =
        sqlx::query_scalar("SELECT count(*) FROM account_upload_reservations WHERE id = $1")
            .bind(held_id)
            .fetch_one(&owner)
            .await
            .expect("count");
    assert_eq!(still, 1, "bob's project released nothing of alice's");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn reconcile_finds_drift_and_repairs_it() {
    let db = TestDb::new().await;
    let alice = db.person("alice").await;
    let one = project(&db, alice, "one").await;
    write(&db, alice, one).await.expect("entry");
    let owner = db.owner().await;
    assert_eq!(
        usage::reconcile(&owner, false).await.expect("in step"),
        Vec::new(),
        "no drift"
    );
    sqlx::query("UPDATE account_usage SET entries = entries + 7 WHERE account = $1")
        .bind(alice)
        .execute(&owner)
        .await
        .expect("drift planted");
    let drift = usage::reconcile(&owner, false).await.expect("checked");
    assert_eq!(drift.len(), 1, "{drift:?}");
    assert_eq!(
        (drift[0].field.as_str(), drift[0].stored, drift[0].counted),
        ("entries", 8, 1)
    );
    // A check changes nothing.
    assert_eq!(
        usage::reconcile(&owner, false).await.expect("again").len(),
        1
    );
    assert_eq!(
        usage::reconcile(&owner, true)
            .await
            .expect("repaired")
            .len(),
        1
    );
    assert_eq!(
        usage::reconcile(&owner, false).await.expect("in step"),
        Vec::new(),
        "no drift"
    );
    db.drop().await;
}
