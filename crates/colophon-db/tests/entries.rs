//! Entries, revisions and idempotency records against real PostgreSQL, as the application's
//! login: the save rule (D2), insert-only revisions (D1), draft pointers that can't cross
//! entries (D3), project isolation, and idempotent replay. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool,
    entries::{self, NewRevision, Saved},
    idempotency::{self, Claim},
    projects,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const ARTICLES: &str = "articles";

/// A revision with made-up content: these tests are about storage, not the article format.
struct Content {
    id: Uuid,
    text: String,
    hash: [u8; 32],
    title: String,
}

impl Content {
    fn new(title: &str) -> Self {
        let id = Uuid::now_v7();
        Self {
            id,
            text: format!(r#"{{"title":"{title}","body":{{}}}}"#),
            hash: *id.as_bytes().repeat(2).first_chunk().expect("32 bytes"),
            title: title.to_owned(),
        }
    }

    fn revision(&self) -> NewRevision<'_> {
        NewRevision {
            id: self.id,
            content: &self.text,
            content_hash: &self.hash,
            title: &self.title,
            schema_version: 1,
        }
    }
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member, in an environment that exists")
}

/// Alice's project with one entry; returns (alice, project, entry, first revision).
async fn alice_with_an_entry(db: &TestDb) -> (Uuid, Uuid, Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let first = Content::new("First");
    let entry = Uuid::now_v7();
    let draft = entries::create(&mut tx, entry, ARTICLES, first.revision())
        .await
        .expect("created");
    assert_eq!(draft.revision.base_revision_id, None);
    tx.commit().await.expect("commit");
    (alice, project, entry, first.id)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_save_on_the_current_draft_moves_it_and_keeps_the_history() {
    let db = TestDb::new().await;
    let (alice, project, entry, first) = alice_with_an_entry(&db).await;

    let second = Content::new("Second");
    let mut tx = enter(&db.app, alice, project).await;
    let Saved::Saved(draft) = entries::save(&mut tx, entry, first, second.revision())
        .await
        .expect("save")
    else {
        panic!("the draft was the expected revision");
    };
    assert_eq!(draft.revision.base_revision_id, Some(first));
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    let current = entries::get(&mut tx, ARTICLES, entry)
        .await
        .expect("get")
        .expect("exists");
    assert_eq!(current.revision.id, second.id);
    assert_eq!(
        current.content, second.text,
        "the stored bytes are the bytes saved"
    );
    let history: Vec<(Uuid, Option<Uuid>)> = entries::history(&mut tx, ARTICLES, entry, 10)
        .await
        .expect("history")
        .into_iter()
        .map(|revision| (revision.id, revision.base_revision_id))
        .collect();
    assert_eq!(history, [(second.id, Some(first)), (first, None)]);
    let listed = entries::list(&mut tx, ARTICLES, None, 20, None)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].revision.title, "Second");
    drop(tx); // closing the pool waits for every connection to come back
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_save_on_a_stale_draft_writes_nothing_and_names_the_current_revision() {
    let db = TestDb::new().await;
    let (alice, project, entry, first) = alice_with_an_entry(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let second = Content::new("Second");
    entries::save(&mut tx, entry, first, second.revision())
        .await
        .expect("save");
    tx.commit().await.expect("commit");

    // Another editor still holds the first revision.
    let mut tx = enter(&db.app, alice, project).await;
    let late = Content::new("Late");
    let outcome = entries::save(&mut tx, entry, first, late.revision())
        .await
        .expect("save");
    assert_eq!(outcome, Saved::Stale { current: second.id });
    let missing = entries::save(&mut tx, Uuid::now_v7(), first, late.revision())
        .await
        .expect("save");
    assert_eq!(missing, Saved::NotFound);
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        entries::history(&mut tx, ARTICLES, entry, 10)
            .await
            .expect("history")
            .len(),
        2,
        "the stale save stored no revision"
    );
    drop(tx); // closing the pool waits for every connection to come back
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_concurrent_saves_on_one_base_revision_one_wins_one_is_stale() {
    let db = TestDb::new().await;
    let (alice, project, entry, first) = alice_with_an_entry(&db).await;
    let pool = db.app_pool(2).await;

    let (a, b) = (Content::new("From A"), Content::new("From B"));
    let mut tx_a = enter(&pool, alice, project).await;
    let mut tx_b = enter(&pool, alice, project).await;
    let saved_a = entries::save(&mut tx_a, entry, first, a.revision())
        .await
        .expect("save");
    assert!(matches!(saved_a, Saved::Saved(_)));
    // B blocks on A's row lock; it must see A's commit, not the old pointer.
    let b_id = b.id;
    let waiting = tokio::spawn(async move {
        let outcome = entries::save(&mut tx_b, entry, first, b.revision())
            .await
            .expect("save");
        tx_b.commit().await.expect("commit");
        outcome
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(!waiting.is_finished(), "B waits for A's lock");
    tx_a.commit().await.expect("commit");
    assert_eq!(waiting.await.expect("task"), Saved::Stale { current: a.id });

    let mut tx = enter(&pool, alice, project).await;
    let ids: Vec<Uuid> = entries::history(&mut tx, ARTICLES, entry, 10)
        .await
        .expect("history")
        .into_iter()
        .map(|revision| revision.id)
        .collect();
    assert_eq!(ids, [a.id, first], "B's revision was never stored: {b_id}");
    drop(tx);
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn revisions_cannot_be_changed_or_deleted_and_pointers_cannot_cross_entries() {
    let db = TestDb::new().await;
    let (alice, project, entry, first) = alice_with_an_entry(&db).await;

    for sql in [
        "UPDATE revisions SET title = 'rewritten' WHERE id = $1",
        "DELETE FROM revisions WHERE id = $1",
        "UPDATE entries SET collection = 'articles' WHERE draft_revision_id = $1",
        "DELETE FROM entries WHERE draft_revision_id = $1",
    ] {
        let mut tx = enter(&db.app, alice, project).await;
        let error = sqlx::query(sql)
            .bind(first)
            .execute(&mut *tx.transaction)
            .await
            .expect_err("no grant");
        assert!(
            error.to_string().contains("permission denied"),
            "{sql}: {error}"
        );
    }

    // A second entry; then try to point the first entry's draft at the second's revision.
    let mut tx = enter(&db.app, alice, project).await;
    let other = Content::new("Other");
    entries::create(&mut tx, Uuid::now_v7(), ARTICLES, other.revision())
        .await
        .expect("created");
    tx.commit().await.expect("commit");
    let mut tx = enter(&db.app, alice, project).await;
    sqlx::query("UPDATE entries SET draft_revision_id = $1 WHERE id = $2")
        .bind(other.id)
        .bind(entry)
        .execute(&mut *tx.transaction)
        .await
        .expect("the update itself runs; the deferred key is checked at commit");
    let error = tx.commit().await.expect_err("refused at commit");
    assert!(
        format!("{error:?}").contains("entries_draft_is_own_revision"),
        "{error:?}"
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn another_project_cannot_see_or_reach_an_entry() {
    let db = TestDb::new().await;
    let (alice, project, entry, _first) = alice_with_an_entry(&db).await;
    let bob = db.administrator("bob").await;
    let beta = projects::create(&db.app, bob, "beta", "Beta")
        .await
        .expect("created")
        .id;

    assert!(
        scope::begin_in_project(&db.app, bob, project, "production")
            .await
            .expect("begin")
            .is_none(),
        "bob is not a member of alpha"
    );
    assert!(
        scope::begin_in_project(&db.app, alice, project, "staging")
            .await
            .expect("begin")
            .is_none(),
        "no such environment"
    );

    // Inside his own project, Bob asks for Alice's entry by its id, and counts every row.
    let mut tx = enter(&db.app, bob, beta).await;
    assert_eq!(
        entries::get(&mut tx, ARTICLES, entry).await.expect("get"),
        None
    );
    for table in ["entries", "revisions", "idempotency_records"] {
        let (n,): (i64,) =
            sqlx::query_as(sqlx::AssertSqlSafe(format!("SELECT count(*) FROM {table}")))
                .fetch_one(&mut *tx.transaction)
                .await
                .expect("count");
        assert_eq!(n, 0, "{table}");
    }
    drop(tx); // closing the pool waits for every connection to come back
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_idempotency_key_replays_its_answer_and_refuses_a_different_request() {
    let db = TestDb::new().await;
    let (alice, project, _entry, _first) = alice_with_an_entry(&db).await;
    let key = "key-0123456789abcdef";
    let (hash, other) = ([1_u8; 32], [2_u8; 32]);

    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        idempotency::claim(&mut tx, "entry.create", key, &hash)
            .await
            .expect("claim"),
        Claim::Fresh
    );
    idempotency::complete(&mut tx, "entry.create", key, 201, r#"{"data":1}"#)
        .await
        .expect("complete");
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        idempotency::claim(&mut tx, "entry.create", key, &hash)
            .await
            .expect("claim"),
        Claim::Replay {
            status: 201,
            response: r#"{"data":1}"#.to_owned()
        }
    );
    assert_eq!(
        idempotency::claim(&mut tx, "entry.create", key, &other)
            .await
            .expect("claim"),
        Claim::Mismatch
    );
    assert_eq!(
        idempotency::claim(&mut tx, "entry.save", key, &other)
            .await
            .expect("claim"),
        Claim::Fresh,
        "keys are per operation"
    );
    drop(tx);

    // Keys are per person: Bob, a member too, can use the same key for his own request.
    let bob = db.person("bob").await;
    let owner = db.owner().await;
    sqlx::query(
        "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'editor')",
    )
    .bind(project)
    .bind(bob)
    .execute(&owner)
    .await
    .expect("bob joins");
    owner.close().await;
    let mut tx = enter(&db.app, bob, project).await;
    assert_eq!(
        idempotency::claim(&mut tx, "entry.create", key, &other)
            .await
            .expect("claim"),
        Claim::Fresh
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_concurrent_duplicate_waits_for_the_first_and_replays_it() {
    let db = TestDb::new().await;
    let (alice, project, _entry, _first) = alice_with_an_entry(&db).await;
    let pool = db.app_pool(2).await;
    let key = "dup-0123456789abcdef";
    let hash = [3_u8; 32];

    let mut first = enter(&pool, alice, project).await;
    assert_eq!(
        idempotency::claim(&mut first, "entry.create", key, &hash)
            .await
            .expect("claim"),
        Claim::Fresh
    );
    let mut second = enter(&pool, alice, project).await;
    let waiting = tokio::spawn(async move {
        idempotency::claim(&mut second, "entry.create", key, &hash)
            .await
            .expect("claim")
    });
    tokio::time::sleep(std::time::Duration::from_millis(200)).await;
    assert!(
        !waiting.is_finished(),
        "the duplicate waits for the first claim"
    );
    idempotency::complete(&mut first, "entry.create", key, 201, "{}")
        .await
        .expect("complete");
    first.commit().await.expect("commit");
    assert_eq!(
        waiting.await.expect("task"),
        Claim::Replay {
            status: 201,
            response: "{}".to_owned()
        }
    );
    pool.close().await;
    db.drop().await;
}
