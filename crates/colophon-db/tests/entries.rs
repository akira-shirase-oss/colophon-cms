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
    let draft = entries::create(&mut tx, entry, ARTICLES, first.revision(), &[], None)
        .await
        .expect("created");
    assert_eq!(draft.revision.base_revision_id, None);
    tx.commit().await.expect("commit");
    (alice, project, entry, first.id)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_collection_exports_each_draft_as_a_line() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    entries::create(
        &mut tx,
        Uuid::now_v7(),
        ARTICLES,
        Content::new("First").revision(),
        &[],
        None,
    )
    .await
    .expect("created");
    entries::create(
        &mut tx,
        Uuid::now_v7(),
        ARTICLES,
        Content::new("Second").revision(),
        &[],
        None,
    )
    .await
    .expect("created");
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    let lines = entries::export(&mut tx, ARTICLES).await.expect("exported");
    assert_eq!(lines.len(), 2);
    // Newest first: each line is the draft's canonical content.
    assert!(lines[0].contains(r#""title":"Second""#), "{}", lines[0]);
    assert!(lines[1].contains(r#""title":"First""#), "{}", lines[1]);
    tx.commit().await.expect("commit");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_unique_value_can_only_be_held_by_one_entry() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let same = [("email".to_owned(), [9_u8; 32])];

    let mut tx = enter(&db.app, alice, project).await;
    entries::create(
        &mut tx,
        Uuid::now_v7(),
        ARTICLES,
        Content::new("A").revision(),
        &same,
        None,
    )
    .await
    .expect("created");
    tx.commit().await.expect("commit");

    // The index refuses the same value under a second entry, whatever changed it.
    let mut tx = enter(&db.app, alice, project).await;
    let error = entries::create(
        &mut tx,
        Uuid::now_v7(),
        ARTICLES,
        Content::new("B").revision(),
        &same,
        None,
    )
    .await
    .expect_err("the value is already held");
    assert!(matches!(error, colophon_db::DbError::Conflict));
    // A refused write aborts the transaction; drop it so the next one can begin.
    drop(tx);

    let mut tx = enter(&db.app, alice, project).await;
    let different = [("email".to_owned(), [8_u8; 32])];
    let entry = Uuid::now_v7();
    entries::create(
        &mut tx,
        entry,
        ARTICLES,
        Content::new("B").revision(),
        &different,
        None,
    )
    .await
    .expect("a different value is fine");
    tx.commit().await.expect("commit");

    // Editing back onto the held value is refused too.
    let held = entries::get(&mut enter(&db.app, alice, project).await, ARTICLES, entry)
        .await
        .expect("get")
        .expect("exists");
    let mut tx = enter(&db.app, alice, project).await;
    let error = entries::save(
        &mut tx,
        entry,
        held.revision.id,
        Content::new("B2").revision(),
        &same,
    )
    .await
    .expect_err("the value is already held");
    assert!(matches!(error, colophon_db::DbError::Conflict));
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_draft_entry_can_be_deleted_and_then_is_gone() {
    let db = TestDb::new().await;
    let (alice, project, entry, _first) = alice_with_an_entry(&db).await;

    let mut tx = enter(&db.app, alice, project).await;
    let removed = entries::remove(&mut tx, ARTICLES, entry)
        .await
        .expect("remove");
    assert!(matches!(removed, entries::Removed::Removed));
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    let again = entries::remove(&mut tx, ARTICLES, entry)
        .await
        .expect("remove");
    assert!(matches!(again, entries::Removed::NotFound));
    let gone = entries::get(&mut tx, ARTICLES, entry).await.expect("get");
    assert!(gone.is_none());
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_save_on_the_current_draft_moves_it_and_keeps_the_history() {
    let db = TestDb::new().await;
    let (alice, project, entry, first) = alice_with_an_entry(&db).await;

    let second = Content::new("Second");
    let mut tx = enter(&db.app, alice, project).await;
    let Saved::Saved(draft) = entries::save(&mut tx, entry, first, second.revision(), &[])
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
    let columns = ["title".to_string(), "missing".to_string()];
    let listed = entries::list(&mut tx, ARTICLES, None, 20, None, &columns, None)
        .await
        .expect("list");
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].revision.title, "Second");
    assert_eq!(
        listed[0].fields, r#"{"title": "Second", "missing": null}"#,
        "the draft's values of the asked-for fields, null for one it doesn't have"
    );
    // Search reads the entry's copy of its draft's title (migration 0015), which moved with it.
    let found = entries::list(&mut tx, ARTICLES, None, 20, Some("ECON"), &[], None)
        .await
        .expect("search");
    assert_eq!(found.len(), 1);
    assert!(
        entries::list(&mut tx, ARTICLES, None, 20, Some("First"), &[], None)
            .await
            .expect("search")
            .is_empty(),
        "an older version's title doesn't match"
    );
    drop(tx); // closing the pool waits for every connection to come back
    let owner = db.owner().await;
    let (mismatched,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.draft_title <> r.title",
    )
    .fetch_one(&owner)
    .await
    .expect("count");
    assert_eq!(mismatched, 0, "draft_title always copies the draft's title");
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_save_on_a_stale_draft_writes_nothing_and_names_the_current_revision() {
    let db = TestDb::new().await;
    let (alice, project, entry, first) = alice_with_an_entry(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let second = Content::new("Second");
    entries::save(&mut tx, entry, first, second.revision(), &[])
        .await
        .expect("save");
    tx.commit().await.expect("commit");

    // Another editor still holds the first revision.
    let mut tx = enter(&db.app, alice, project).await;
    let late = Content::new("Late");
    let outcome = entries::save(&mut tx, entry, first, late.revision(), &[])
        .await
        .expect("save");
    assert_eq!(outcome, Saved::Stale { current: second.id });
    let missing = entries::save(&mut tx, Uuid::now_v7(), first, late.revision(), &[])
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
    let saved_a = entries::save(&mut tx_a, entry, first, a.revision(), &[])
        .await
        .expect("save");
    assert!(matches!(saved_a, Saved::Saved(_)));
    // B blocks on A's row lock; it must see A's commit, not the old pointer.
    let b_id = b.id;
    let waiting = tokio::spawn(async move {
        let outcome = entries::save(&mut tx_b, entry, first, b.revision(), &[])
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

    // UPDATE stays ungranted — a stored revision is immutable. DELETE is granted for the
    // removal flow, so the foreign keys do the protecting, possibly deferred to commit.
    for sql in [
        "UPDATE revisions SET title = 'rewritten' WHERE id = $1",
        "UPDATE entries SET collection = 'articles' WHERE draft_revision_id = $1",
    ] {
        let mut tx = enter(&db.app, alice, project).await;
        let error = sqlx::query(sql)
            .bind(first)
            .execute(&mut *tx.transaction)
            .await
            .expect_err("no update grant");
        assert!(
            error.to_string().contains("permission denied"),
            "{sql}: {error}"
        );
    }
    for sql in [
        "DELETE FROM revisions WHERE id = $1",
        "DELETE FROM entries WHERE draft_revision_id = $1",
    ] {
        let mut tx = enter(&db.app, alice, project).await;
        let error = match sqlx::query(sql)
            .bind(first)
            .execute(&mut *tx.transaction)
            .await
        {
            Err(error) => format!("{error:?}"),
            Ok(_) => format!("{:?}", tx.commit().await.expect_err("refused at commit")),
        };
        assert!(error.contains("foreign key"), "{sql}: {error}");
    }

    // A second entry; then try to point the first entry's draft at the second's revision.
    let mut tx = enter(&db.app, alice, project).await;
    let other = Content::new("Other");
    entries::create(
        &mut tx,
        Uuid::now_v7(),
        ARTICLES,
        other.revision(),
        &[],
        None,
    )
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

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn referencing_screens_drafts_that_mention_an_entry_id() {
    let db = TestDb::new().await;
    let (alice, project, entry, _first) = alice_with_an_entry(&db).await;
    // A second article whose draft happens to contain the first entry's id.
    let mut tx = enter(&db.app, alice, project).await;
    let hash = [9_u8; 32];
    let content = format!(r#"{{"title":"Referrer","about":"{entry}"}}"#);
    let referrer = Uuid::now_v7();
    entries::create(
        &mut tx,
        referrer,
        ARTICLES,
        NewRevision {
            id: Uuid::now_v7(),
            content: &content,
            content_hash: &hash,
            title: "Referrer",
            schema_version: 1,
        },
        &[],
        None,
    )
    .await
    .expect("created");
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    let found = entries::referencing(&mut tx, ARTICLES, entry)
        .await
        .expect("scan");
    assert_eq!(found.len(), 1);
    assert_eq!(
        (found[0].id, found[0].title.as_str()),
        (referrer, "Referrer")
    );
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn an_entrys_locale_is_set_filtered_and_rewritten() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let first = Content::new("English");
    let english = Uuid::now_v7();
    entries::create(
        &mut tx,
        english,
        ARTICLES,
        first.revision(),
        &[],
        Some("en"),
    )
    .await
    .expect("created");
    let french = Uuid::now_v7();
    entries::create(
        &mut tx,
        french,
        ARTICLES,
        Content::new("French").revision(),
        &[],
        Some("fr"),
    )
    .await
    .expect("created");
    let plain = Uuid::now_v7();
    entries::create(
        &mut tx,
        plain,
        ARTICLES,
        Content::new("Plain").revision(),
        &[],
        None,
    )
    .await
    .expect("created");

    // The list filter keeps only the asked-for locale; no filter sees all three.
    let english_only = entries::list(&mut tx, ARTICLES, None, 20, None, &[], Some("en"))
        .await
        .expect("listed");
    assert_eq!(
        english_only
            .iter()
            .map(|row| row.entry_id)
            .collect::<Vec<_>>(),
        vec![english]
    );
    let all = entries::list(&mut tx, ARTICLES, None, 20, None, &[], None)
        .await
        .expect("listed");
    assert_eq!(all.len(), 3);

    // Setting the locale moves the tag; clearing it drops the entry from the filter.
    assert!(
        entries::set_locale(&mut tx, ARTICLES, plain, Some("de"))
            .await
            .expect("set")
    );
    let loaded = entries::get(&mut tx, ARTICLES, plain)
        .await
        .expect("get")
        .expect("there");
    assert_eq!(loaded.locale.as_deref(), Some("de"));
    let german = entries::list(&mut tx, ARTICLES, None, 20, None, &[], Some("de"))
        .await
        .expect("listed");
    assert_eq!(german.len(), 1);
    assert!(
        entries::set_locale(&mut tx, ARTICLES, plain, None)
            .await
            .expect("cleared")
    );
    let german = entries::list(&mut tx, ARTICLES, None, 20, None, &[], Some("de"))
        .await
        .expect("listed");
    assert_eq!(german, []);
}
