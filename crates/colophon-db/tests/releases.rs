//! Releases and delivery against real PostgreSQL (D-045): a release commits only from its base
//! generation, publication history can't overlap even when written by hand, and the delivery
//! role reads published content and nothing else. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, changesets, delivery,
    entries::{self, NewRevision},
    projects,
    releases::{self, Committed},
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const HOUR: std::time::Duration = std::time::Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A project with one article pinned into an approved changeset; (project, entry, changeset, review).
async fn ready(db: &TestDb) -> (Uuid, Uuid, Uuid, Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: r#"{"title":"Hello"}"#,
            content_hash: &[3; 32],
            title: "Hello",
            schema_version: 1,
        },
    )
    .await
    .expect("created");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "One", &pins, &[5; 32])
        .await
        .expect("created");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[5; 32], "approve", "")
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    tx.commit().await.expect("commit");
    (alice, project, entry, changeset, review)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_release_commits_once_from_its_base_and_history_never_overlaps() {
    let db = TestDb::new().await;
    let (alice, project, entry, changeset, review) = ready(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        releases::commit(&mut tx, 3, changeset, &[5; 32], review, HOUR)
            .await
            .expect("commit"),
        Committed::Moved { current: 0 },
        "a base the environment never had"
    );
    assert_eq!(
        releases::commit(&mut tx, 0, changeset, &[5; 32], review, HOUR)
            .await
            .expect("commit"),
        Committed::Done {
            generation: 1,
            epoch: 1,
            changed: 1,
            unpublished: 0
        }
    );
    tx.commit().await.expect("commit");

    // Below the API: an overlapping interval is refused by the database itself.
    let owner = db.owner().await;
    let error = sqlx::query(
        "INSERT INTO publications (project_id, environment_id, entry_id, revision_id, collection, title, content, content_hash, valid_from)
         SELECT project_id, environment_id, entry_id, revision_id, collection, title, content, content_hash, valid_from
         FROM publications WHERE entry_id = $1",
    )
    .bind(entry)
    .execute(&owner)
    .await
    .expect_err("a second open interval for one entry");
    assert!(
        format!("{error:?}").contains("23P01")
            || format!("{error:?}").contains("exclusion")
            || format!("{error:?}").contains("duplicate"),
        "{error:?}"
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn delivery_reads_published_content_and_nothing_else() {
    let db = TestDb::new().await;
    let (alice, project, entry, changeset, review) = ready(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    releases::commit(&mut tx, 0, changeset, &[5; 32], review, HOUR)
        .await
        .expect("commit");
    tx.commit().await.expect("commit");

    let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
    let environment = delivered
        .environment("production")
        .await
        .expect("environment")
        .expect("found");
    assert_eq!(environment.generation, 1);
    let published = delivered
        .list(environment.id, "articles", 1, None, 10)
        .await
        .expect("list");
    assert_eq!(
        published.iter().map(|row| row.entry_id).collect::<Vec<_>>(),
        [entry]
    );
    delivered.finish().await.expect("finish");

    for statement in [
        "SELECT count(*) FROM revisions",
        "SELECT count(*) FROM entries",
        "SELECT count(*) FROM changesets",
        "SELECT count(*) FROM memberships",
        "SELECT count(*) FROM sessions",
        "SELECT count(*) FROM identities",
        "UPDATE publications SET title = 'defaced'",
        "UPDATE environments SET generation = 99",
    ] {
        let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
        let error = sqlx::query(statement)
            .execute(&mut **delivered.raw())
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{statement}: {error:?}"
        );
    }

    // Another project's id sees nothing of this one.
    let mut elsewhere = delivery::begin(&db.app, Uuid::now_v7())
        .await
        .expect("begin");
    assert_eq!(
        elsewhere
            .environment("production")
            .await
            .expect("environment"),
        None
    );
    elsewhere.finish().await.expect("finish");
    db.drop().await;
}

/// The delivery cache's one-statement generation lookup (migration 0011, D-048): it answers for the
/// project asked about and nothing else, the project context it sets doesn't outlive the call, and
/// only the API's role may call it.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_generation_lookup_answers_for_one_project_and_leaves_nothing_behind() {
    let db = TestDb::new().await;
    let (alice, project, _, changeset, review) = ready(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    releases::commit(&mut tx, 0, changeset, &[5; 32], review, HOUR)
        .await
        .expect("commit");
    tx.commit().await.expect("commit");

    let found = delivery::environment(&db.app, project, "production")
        .await
        .expect("looked up")
        .expect("found");
    assert_eq!((found.generation, found.epoch), (1, 1));
    let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
    let same = delivered
        .environment("production")
        .await
        .expect("environment");
    delivered.finish().await.expect("finish");
    assert_eq!(
        same,
        Some(found),
        "the same answer as a delivery transaction"
    );
    for (other_project, slug) in [(Uuid::now_v7(), "production"), (project, "staging")] {
        assert_eq!(
            delivery::environment(&db.app, other_project, slug)
                .await
                .expect("looked up"),
            None
        );
    }

    // Called inside a transaction, it leaves no project context behind.
    let mut held = db.app.begin().await.expect("begin");
    let leftover: Option<String> = sqlx::query_scalar(
        "SELECT (SELECT generation FROM delivery_environment($1, 'production'))::text \
         || ':' || coalesce(nullif(current_setting('app.project_id', true), ''), 'none')",
    )
    .bind(project)
    .fetch_one(&mut *held)
    .await
    .expect("called");
    assert_eq!(leftover.as_deref(), Some("1:none"));
    held.rollback().await.expect("rollback");

    // Only the API's role may call it: not the worker's.
    let mut worker = db.app.begin().await.expect("begin");
    sqlx::query("SET LOCAL ROLE colophon_worker")
        .execute(&mut *worker)
        .await
        .expect("switch");
    let refused = sqlx::query("SELECT * FROM delivery_environment($1, 'production')")
        .bind(project)
        .execute(&mut *worker)
        .await
        .expect_err("the worker can't call it");
    assert!(
        format!("{refused:?}").contains("permission denied"),
        "{refused:?}"
    );
    worker.rollback().await.expect("rollback");
    db.drop().await;
}

/// A changeset of `pins`, submitted and approved; (changeset, review).
async fn approved(tx: &mut ProjectTransaction, pins: &[changesets::Pin]) -> (Uuid, Uuid) {
    let changeset = Uuid::now_v7();
    changesets::create(tx, changeset, "Changes", pins, &[6; 32])
        .await
        .expect("created");
    changesets::submit(tx, changeset).await.expect("submitted");
    changesets::lock(tx, changeset).await.expect("lock");
    changesets::review(tx, changeset, &[6; 32], "approve", "")
        .await
        .expect("approved");
    let review = changesets::reviews(tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    (changeset, review)
}

/// What delivery shows of `entry` at `generation`: (in the list, found, gone).
async fn delivered_at(
    db: &TestDb,
    project: Uuid,
    entry: Uuid,
    generation: i64,
) -> (bool, bool, bool) {
    let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
    let environment = delivered
        .environment("production")
        .await
        .expect("environment")
        .expect("found");
    let listed = delivered
        .list(environment.id, "articles", generation, None, 10)
        .await
        .expect("list")
        .iter()
        .any(|row| row.entry_id == entry);
    let found = delivered
        .get(environment.id, "articles", entry, generation)
        .await
        .expect("get")
        .is_some();
    let gone = delivered
        .gone(environment.id, "articles", entry, generation)
        .await
        .expect("gone");
    delivered.finish().await.expect("finish");
    (listed, found, gone)
}

/// Unpublishing (D-049, DL5): the release closes the entry's publication, leaves a tombstone and
/// revokes every earlier publication, so no generation delivers it; an unpublish whose pinned
/// revision is no longer the published one commits nothing; publishing the entry again delivers it
/// from its new generation only.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: published, taken down, a stale takedown, published again"
)]
async fn unpublishing_takes_an_entry_out_of_every_earlier_release_until_it_is_published_again() {
    let db = TestDb::new().await;
    let (alice, project, entry, changeset, review) = ready(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    releases::commit(&mut tx, 0, changeset, &[5; 32], review, HOUR)
        .await
        .expect("commit");
    // Two takedowns of the same publication, both approved while it was live.
    let pins = changesets::published_pins(&mut tx, &[entry])
        .await
        .expect("pins");
    assert_eq!(pins.len(), 1);
    assert_eq!(pins[0].kind, "unpublish");
    let (first, first_review) = approved(&mut tx, &pins).await;
    let (second, second_review) = approved(&mut tx, &pins).await;
    tx.commit().await.expect("commit");
    assert_eq!(
        delivered_at(&db, project, entry, 1).await,
        (true, true, false)
    );

    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        releases::commit(&mut tx, 1, first, &[6; 32], first_review, HOUR)
            .await
            .expect("commit"),
        Committed::Done {
            generation: 2,
            epoch: 1,
            changed: 0,
            unpublished: 1
        }
    );
    let items = changesets::items(&mut tx, second).await.expect("items");
    assert!(
        items[0].moved(),
        "nothing is published for the second one to take down"
    );
    assert_eq!(
        changesets::published_pins(&mut tx, &[entry])
            .await
            .expect("pins"),
        [],
        "nothing left to unpublish"
    );
    tx.commit().await.expect("commit");
    for generation in [1, 2] {
        assert_eq!(
            delivered_at(&db, project, entry, generation).await,
            (false, false, true),
            "generation {generation}"
        );
    }
    let environment = delivery::environment(&db.app, project, "production")
        .await
        .expect("looked up")
        .expect("found");
    assert_eq!((environment.generation, environment.last_unpublish), (2, 2));

    // The second takedown no longer matches what is published: nothing commits.
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        releases::commit(&mut tx, 2, second, &[6; 32], second_review, HOUR)
            .await
            .expect("commit"),
        Committed::Unmatched
    );
    // The caller drops the transaction, which rolls the bumped generation back.
    drop(tx);
    let environment = delivery::environment(&db.app, project, "production")
        .await
        .expect("looked up")
        .expect("found");
    assert_eq!(environment.generation, 2);

    // Published again: delivered from generation 3 on, and still not through 1's or 2's links.
    let mut tx = enter(&db.app, alice, project).await;
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let (again, again_review) = approved(&mut tx, &pins).await;
    assert_eq!(
        releases::commit(&mut tx, 2, again, &[6; 32], again_review, HOUR)
            .await
            .expect("commit"),
        Committed::Done {
            generation: 3,
            epoch: 1,
            changed: 1,
            unpublished: 0
        }
    );
    let listed = releases::list(&mut tx).await.expect("releases");
    assert_eq!(
        listed
            .iter()
            .map(|release| (release.generation, release.entries, release.unpublished))
            .collect::<Vec<_>>(),
        [(3, 1, 0), (2, 0, 1), (1, 1, 0)]
    );
    tx.commit().await.expect("commit");
    assert_eq!(
        delivered_at(&db, project, entry, 3).await,
        (true, true, false)
    );
    assert_eq!(
        delivered_at(&db, project, entry, 1).await,
        (false, false, true)
    );

    // History stays, marked: the first publication was revoked by release 2.
    let owner = db.owner().await;
    let history: Vec<(i64, Option<i64>, Option<i64>)> = sqlx::query_as(
        "SELECT valid_from, valid_to, revoked_in FROM publications WHERE entry_id = $1 ORDER BY valid_from",
    )
    .bind(entry)
    .fetch_all(&owner)
    .await
    .expect("history");
    assert_eq!(history, [(1, Some(2), Some(2)), (3, None, None)]);
    owner.close().await;

    // The delivery role reads tombstones and can't touch them.
    for statement in [
        "DELETE FROM tombstones",
        "UPDATE publications SET revoked_in = NULL",
        "UPDATE environments SET last_unpublish = 0",
    ] {
        let mut delivered = delivery::begin(&db.app, project).await.expect("begin");
        let error = sqlx::query(statement)
            .execute(&mut **delivered.raw())
            .await
            .expect_err(statement);
        assert!(
            format!("{error:?}").contains("permission denied"),
            "{statement}: {error:?}"
        );
    }
    db.drop().await;
}
