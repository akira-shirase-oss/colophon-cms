//! Deleting entries against real PostgreSQL, as the application's login (migration 0030, D-080):
//! a deletion leaves a tombstone the application no longer sees while revisions, history and the
//! audit log keep it; it is refused while the draft moved on, while readers get the entry, while
//! another live entry names it, and while an unpublished changeset pins it; what it held comes
//! free; and the database, not a precheck, decides the races with a new reference, a new pin and a
//! new publication, each run with two real transactions in both orders. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    DbError, PgPool, changesets, content_types,
    entries::{self, Deleted, NewRevision, Pending, Saved},
    projects,
    releases::{self, Committed},
    scope::{self, ProjectTransaction},
};
use colophon_domain::schema::ContentReference;
use common::TestDb;
use uuid::Uuid;

const ARTICLES: &str = "articles";
const HOUR: Duration = Duration::from_secs(3600);
const PAGES: &str = r#"{"name":"Pages","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false},{"id":"slug","label":"URL name","type":"slug","required":false},{"id":"code","label":"Code","type":"text","required":false,"unique":true}]}"#;
const SETTINGS: &str = r#"{"name":"Settings","single":true,"title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false}]}"#;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// Alice, an administrator, and her project.
async fn alpha(db: &TestDb) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    (alice, project)
}

/// What a test stores: the database layer keeps what the API validated, so only the columns the
/// rules read matter.
struct Draft {
    id: Uuid,
    content: String,
    title: String,
    slug: Option<String>,
    references: Vec<ContentReference>,
    unique: Vec<String>,
}

impl Draft {
    fn new(title: &str) -> Self {
        Self {
            id: Uuid::now_v7(),
            content: format!(r#"{{"title":"{title}"}}"#),
            title: title.to_owned(),
            slug: None,
            references: Vec::new(),
            unique: Vec::new(),
        }
    }

    /// Names `entry` (an article) in its `related` field.
    fn naming(mut self, entry: Uuid) -> Self {
        self.references.push(ContentReference {
            field: "related".to_owned(),
            collection: ARTICLES.to_owned(),
            position: 0,
            many: false,
            entry: entry.to_string(),
        });
        self
    }

    /// A page with this slug and this unique code.
    fn page(title: &str, slug: &str, code: &str) -> Self {
        Self {
            content: format!(r#"{{"code":"{code}","slug":"{slug}","title":"{title}"}}"#),
            slug: Some(slug.to_owned()),
            unique: vec!["code".to_owned()],
            ..Self::new(title)
        }
    }

    fn revision(&self) -> NewRevision<'_> {
        NewRevision {
            id: self.id,
            content: &self.content,
            content_hash: &[7; 32],
            title: &self.title,
            slug: self.slug.as_deref(),
            schema_version: 1,
            references: &self.references,
            unique: &self.unique,
        }
    }
}

/// Creates an entry of `collection` from `draft`; its id and first revision.
async fn create(
    tx: &mut ProjectTransaction,
    collection: &str,
    draft: &Draft,
) -> Result<(Uuid, Uuid), DbError> {
    let entry = Uuid::now_v7();
    entries::create(tx, entry, collection, draft.revision()).await?;
    Ok((entry, draft.id))
}

/// Saves `draft` on top of `base`; the new revision.
async fn save(
    tx: &mut ProjectTransaction,
    entry: Uuid,
    base: Uuid,
    draft: &Draft,
) -> Result<Uuid, DbError> {
    match entries::save(tx, entry, base, draft.revision()).await? {
        Saved::Saved(_) => Ok(draft.id),
        other => panic!("expected a save, got {other:?}"),
    }
}

/// A new changeset pinning `pins`, submitted and approved; its id and approval.
async fn approved(tx: &mut ProjectTransaction, pins: &[changesets::Pin]) -> (Uuid, Uuid) {
    let changeset = Uuid::now_v7();
    changesets::create(tx, changeset, "Deletion test", pins, &[9; 32])
        .await
        .expect("changeset");
    changesets::submit(tx, changeset).await.expect("submit");
    changesets::lock(tx, changeset).await.expect("lock");
    changesets::review(tx, changeset, &[9; 32], "approve", "")
        .await
        .expect("review");
    let review = changesets::reviews(tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    (changeset, review)
}

/// Commits `changeset` as the next release.
async fn commit(
    tx: &mut ProjectTransaction,
    changeset: Uuid,
    review: Uuid,
) -> Result<Committed, DbError> {
    let (_, base) = releases::generation(tx).await?;
    releases::commit(tx, base, changeset, &[9; 32], review, HOUR).await
}

/// Waits until some transaction in this test's database waits for a lock: the second of two
/// racing transactions has reached the point where the database makes it queue.
async fn until_one_waits(owner: &PgPool) {
    for _ in 0..250 {
        let (waiting,): (i64,) = sqlx::query_as(
            "SELECT count(*) FROM pg_stat_activity
             WHERE datname = current_database() AND wait_event_type = 'Lock'",
        )
        .fetch_one(owner)
        .await
        .expect("activity");
        if waiting > 0 {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("no transaction ever waited for a lock");
}

fn deleted(outcome: &Deleted) -> Uuid {
    match outcome {
        Deleted::Deleted { revision, .. } => *revision,
        other => panic!("expected a deletion, got {other:?}"),
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_deleted_entry_is_a_tombstone_the_application_no_longer_sees() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, first) = create(&mut tx, ARTICLES, &Draft::new("Doomed"))
        .await
        .expect("created");
    let second = save(&mut tx, entry, first, &Draft::new("Doomed, edited"))
        .await
        .expect("saved");
    let (kept, _) = create(&mut tx, ARTICLES, &Draft::new("Kept"))
        .await
        .expect("created");
    tx.commit().await.expect("commit");

    let mut tx = enter(&db.app, alice, project).await;
    let outcome = entries::delete(&mut tx, ARTICLES, entry, second)
        .await
        .expect("delete");
    assert_eq!(
        deleted(&outcome),
        second,
        "it deleted the draft it was shown"
    );
    tx.commit().await.expect("commit");

    // The application's role sees nothing of it: not by id, not in a list, not to save or
    // delete again. History is read through the entry, which is gone.
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        entries::get(&mut tx, ARTICLES, entry).await.expect("get"),
        None
    );
    let creators = entries::creators(&mut tx, &[entry])
        .await
        .expect("creators");
    assert_eq!(creators, Vec::new());
    let listed: Vec<Uuid> = entries::list(&mut tx, ARTICLES, None, 100, None, None)
        .await
        .expect("list")
        .into_iter()
        .map(|summary| summary.entry_id)
        .collect();
    assert_eq!(listed, vec![kept]);
    let again = Draft::new("Back from the dead");
    assert_eq!(
        entries::save(&mut tx, entry, second, again.revision())
            .await
            .expect("save"),
        Saved::NotFound
    );
    assert_eq!(
        entries::delete(&mut tx, ARTICLES, entry, second)
            .await
            .expect("delete"),
        Deleted::NotFound
    );
    // It can't mark a row deleted itself, or see one through a forged read.
    let refused = sqlx::query("UPDATE entries SET deleted_at = now() WHERE id = $1")
        .bind(kept)
        .execute(&mut *tx.transaction)
        .await
        .expect_err("only colophon_delete_entry marks entries deleted");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("42501"),
        "{refused}"
    );
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    let (seen,): (i64,) = sqlx::query_as("SELECT count(*) FROM entries WHERE id = $1")
        .bind(entry)
        .fetch_one(&mut *tx.transaction)
        .await
        .expect("count");
    assert_eq!(seen, 0);
    drop(tx);

    // The row, its revisions and the record of who deleted it are all still there.
    let owner = db.owner().await;
    let (deleted_by, revisions): (Option<Uuid>, i64) = sqlx::query_as(
        "SELECT e.deleted_by, (SELECT count(*) FROM revisions r WHERE r.entry_id = e.id)
         FROM entries e WHERE e.id = $1 AND e.deleted_at IS NOT NULL",
    )
    .bind(entry)
    .fetch_one(&owner)
    .await
    .expect("the tombstone");
    assert_eq!((deleted_by, revisions), (Some(alice), 2));
    let (actor, detail): (Uuid, String) = sqlx::query_as(
        "SELECT actor_id, detail::text FROM audit_events
         WHERE project_id = $1 AND action = 'entry.deleted' AND subject = $2",
    )
    .bind(project)
    .bind(entry.to_string())
    .fetch_one(&owner)
    .await
    .expect("one audit event");
    assert_eq!(actor, alice);
    assert!(
        detail.contains("Doomed, edited") && detail.contains(&second.to_string()),
        "{detail}"
    );
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_deletion_is_refused_while_anything_depends_on_the_entry() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let (target, target_draft) = create(&mut tx, ARTICLES, &Draft::new("Target"))
        .await
        .expect("created");
    let (fan, fan_first) = create(&mut tx, ARTICLES, &Draft::new("Fan").naming(target))
        .await
        .expect("created");
    // A deleted referrer doesn't count; neither does an entry naming itself.
    let (gone_fan, gone_fan_draft) =
        create(&mut tx, ARTICLES, &Draft::new("Gone fan").naming(target))
            .await
            .expect("created");
    tx.commit().await.expect("commit");
    let mut tx = enter(&db.app, alice, project).await;
    deleted(
        &entries::delete(&mut tx, ARTICLES, gone_fan, gone_fan_draft)
            .await
            .expect("delete"),
    );

    // A stale If-Match, then a live referrer, named.
    let stale = Uuid::now_v7();
    assert_eq!(
        entries::delete(&mut tx, ARTICLES, target, stale)
            .await
            .expect("delete"),
        Deleted::Stale {
            current: target_draft
        }
    );
    match entries::delete(&mut tx, ARTICLES, target, target_draft)
        .await
        .expect("delete")
    {
        Deleted::Referenced(referrers) => {
            let named: Vec<(Uuid, &str, bool)> = referrers
                .iter()
                .map(|r| (r.entry_id, r.field.as_str(), r.in_draft))
                .collect();
            assert_eq!(named, vec![(fan, "related", true)]);
        }
        other => panic!("expected a refusal naming the referrer, got {other:?}"),
    }
    // Once the referrer's draft stops naming it, only an unpublished changeset is in the way.
    save(&mut tx, fan, fan_first, &Draft::new("Fan, moved on"))
        .await
        .expect("saved");
    let pins = changesets::pins(&mut tx, &[target]).await.expect("pins");
    let (changeset, review) = approved(&mut tx, &pins).await;
    assert_eq!(
        entries::delete(&mut tx, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
        Deleted::InChangeset(vec![Pending {
            id: changeset,
            title: "Deletion test".to_owned(),
            scheduled: false,
        }])
    );
    // Published, it must be taken down first.
    assert!(matches!(
        commit(&mut tx, changeset, review).await.expect("commit"),
        Committed::Done { .. }
    ));
    assert_eq!(
        entries::delete(&mut tx, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
        Deleted::Published
    );
    // Taken down by a reviewed release, it may go.
    let down = changesets::published_pins(&mut tx, &[target])
        .await
        .expect("pins");
    let (changeset, review) = approved(&mut tx, &down).await;
    assert!(matches!(
        commit(&mut tx, changeset, review).await.expect("commit"),
        Committed::Done { unpublished: 1, .. }
    ));
    deleted(
        &entries::delete(&mut tx, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
    );
    tx.commit().await.expect("commit");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn what_a_deleted_entry_held_comes_free() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    content_types::create(&mut tx, "pages", PAGES, &[1; 32])
        .await
        .expect("pages");
    content_types::create(&mut tx, "settings", SETTINGS, &[2; 32])
        .await
        .expect("settings");
    let (page, page_draft) = create(&mut tx, "pages", &Draft::page("Home", "home", "A1"))
        .await
        .expect("created");
    let (settings, settings_draft) = create(&mut tx, "settings", &Draft::new("Site"))
        .await
        .expect("created");
    tx.commit().await.expect("commit");

    // While they live, nothing else may take them.
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        create(&mut tx, "pages", &Draft::page("Copy", "home", "B2")).await,
        Err(DbError::SlugTaken)
    ));
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        create(&mut tx, "pages", &Draft::page("Copy", "copy", "A1")).await,
        Err(DbError::ValueTaken)
    ));
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        create(&mut tx, "settings", &Draft::new("Second")).await,
        Err(DbError::SingleExists)
    ));
    drop(tx);

    let mut tx = enter(&db.app, alice, project).await;
    deleted(
        &entries::delete(&mut tx, "pages", page, page_draft)
            .await
            .expect("delete"),
    );
    deleted(
        &entries::delete(&mut tx, "settings", settings, settings_draft)
            .await
            .expect("delete"),
    );
    create(&mut tx, "pages", &Draft::page("New home", "home", "A1"))
        .await
        .expect("the slug and the code are free");
    create(&mut tx, "settings", &Draft::new("New site"))
        .await
        .expect("the single type's place is free");
    tx.commit().await.expect("commit");

    // The tombstone keeps its draft's slug (D-054's copy stays true) but no claims.
    let owner = db.owner().await;
    let (slug, claims): (Option<String>, i64) = sqlx::query_as(
        "SELECT e.draft_slug, (SELECT count(*) FROM unique_claims c WHERE c.entry_id = e.id)
         FROM entries e WHERE e.id = $1",
    )
    .bind(page)
    .fetch_one(&owner)
    .await
    .expect("the tombstone");
    assert_eq!((slug.as_deref(), claims), (Some("home"), 0));
    owner.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_reference_to_a_deleted_entry_is_refused_where_it_is() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let (target, target_draft) = create(&mut tx, ARTICLES, &Draft::new("Target"))
        .await
        .expect("created");
    let (fan, fan_first) = create(&mut tx, ARTICLES, &Draft::new("Fan"))
        .await
        .expect("created");
    deleted(
        &entries::delete(&mut tx, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
    );
    tx.commit().await.expect("commit");
    let mut tx = enter(&db.app, alice, project).await;
    let named = Draft::new("Fan").naming(target);
    match entries::save(&mut tx, fan, fan_first, named.revision()).await {
        Err(DbError::MissingReference {
            field, position, ..
        }) => assert_eq!((field.as_str(), position), ("related", 0)),
        other => panic!("expected a missing reference, got {other:?}"),
    }
    drop(tx);
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "three races, each told in order: deletion first, a raw insert, the save first"
)]
async fn a_deletion_and_a_new_reference_never_both_commit() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let pool = db.app_pool(3).await;
    let owner = db.owner().await;
    let mut tx = enter(&pool, alice, project).await;
    let mut targets = Vec::new();
    for title in ["First", "Second", "Third"] {
        targets.push(
            create(&mut tx, ARTICLES, &Draft::new(title))
                .await
                .expect("created"),
        );
    }
    let (fan, fan_first) = create(&mut tx, ARTICLES, &Draft::new("Fan"))
        .await
        .expect("created");
    tx.commit().await.expect("commit");

    // The deletion holds the entry first: the save's reference waits, then finds it gone.
    let (target, target_draft) = targets[0];
    let mut deleting = enter(&pool, alice, project).await;
    deleted(
        &entries::delete(&mut deleting, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
    );
    let saving = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            let named = Draft::new("Fan").naming(target);
            entries::save(&mut tx, fan, fan_first, named.revision())
                .await
                .map(|_| ())
        })
    };
    until_one_waits(&owner).await;
    assert!(!saving.is_finished(), "the save waits for the deletion");
    deleting.commit().await.expect("commit");
    assert!(matches!(
        saving.await.expect("task"),
        Err(DbError::MissingReference { .. })
    ));

    // The same race, with an insert that skips the save's own check: the trigger is the rule.
    let (target, target_draft) = targets[1];
    let mut deleting = enter(&pool, alice, project).await;
    deleted(
        &entries::delete(&mut deleting, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
    );
    let inserting = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            sqlx::query(
                "INSERT INTO revision_references
                   (project_id, environment_id, revision_id, field, position, collection, entry_id)
                 VALUES ($1, $2, $3, 'sneaky', 0, 'articles', $4)",
            )
            .bind(tx.project)
            .bind(tx.environment)
            .bind(fan_first)
            .bind(target)
            .execute(&mut *tx.transaction)
            .await
            .map(|_| ())
        })
    };
    until_one_waits(&owner).await;
    deleting.commit().await.expect("commit");
    let refused = inserting
        .await
        .expect("task")
        .expect_err("the trigger refuses a reference to a deleted entry");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(|error| error.constraint()),
        Some("revision_references_entry_live"),
        "{refused}"
    );

    // The save holds it first: the deletion waits, then sees the new reference and refuses.
    let (target, target_draft) = targets[2];
    let mut saving = enter(&pool, alice, project).await;
    save(
        &mut saving,
        fan,
        fan_first,
        &Draft::new("Fan").naming(target),
    )
    .await
    .expect("saved");
    let deleting = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            entries::delete(&mut tx, ARTICLES, target, target_draft).await
        })
    };
    until_one_waits(&owner).await;
    assert!(!deleting.is_finished(), "the deletion waits for the save");
    saving.commit().await.expect("commit");
    match deleting.await.expect("task").expect("delete") {
        Deleted::Referenced(referrers) => {
            assert_eq!(
                referrers.iter().map(|r| r.entry_id).collect::<Vec<_>>(),
                vec![fan]
            );
        }
        other => panic!("expected the new reference to refuse it, got {other:?}"),
    }
    owner.close().await;
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "three races, each told in order: pin, publication, and publish first"
)]
async fn a_deletion_and_a_new_pin_or_publication_never_both_commit() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let pool = db.app_pool(3).await;
    let owner = db.owner().await;
    let mut tx = enter(&pool, alice, project).await;
    let mut targets = Vec::new();
    for title in ["Pinned", "Published", "Published first", "Bystander"] {
        targets.push(
            create(&mut tx, ARTICLES, &Draft::new(title))
                .await
                .expect("created"),
        );
    }
    tx.commit().await.expect("commit");

    // A changeset being made pins the entry while it is being deleted: the pin waits, then fails.
    let (target, target_draft) = targets[0];
    let mut deleting = enter(&pool, alice, project).await;
    deleted(
        &entries::delete(&mut deleting, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
    );
    let pinning = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            // Read before the deletion commits, as a request already under way would have.
            let pins = changesets::pins(&mut tx, &[target]).await.expect("pins");
            assert_eq!(pins.len(), 1, "the deletion hasn't committed yet");
            changesets::create(&mut tx, Uuid::now_v7(), "Late", &pins, &[9; 32]).await
        })
    };
    until_one_waits(&owner).await;
    deleting.commit().await.expect("commit");
    assert!(matches!(
        pinning.await.expect("task"),
        Err(DbError::EntryDeleted { entry: Some(entry) }) if entry == target
    ));

    // A release that would publish an entry deleted meanwhile is refused, precisely. The pin
    // can't happen through the application (above), so the table owner makes one with triggers
    // off, in an approved changeset that also publishes a bystander.
    let (target, target_draft) = targets[1];
    let (bystander, _) = targets[3];
    let mut tx = enter(&pool, alice, project).await;
    let pins = changesets::pins(&mut tx, &[bystander]).await.expect("pins");
    let (changeset, review) = approved(&mut tx, &pins).await;
    tx.commit().await.expect("commit");
    let mut deleting = enter(&pool, alice, project).await;
    deleted(
        &entries::delete(&mut deleting, ARTICLES, target, target_draft)
            .await
            .expect("delete"),
    );
    let mut bypass = owner.begin().await.expect("begin");
    sqlx::query("SET LOCAL session_replication_role = replica")
        .execute(&mut *bypass)
        .await
        .expect("triggers off");
    sqlx::query(
        "INSERT INTO changeset_items (project_id, environment_id, changeset_id, entry_id, revision_id)
         SELECT project_id, environment_id, $1, $2, $3 FROM changesets WHERE id = $1",
    )
    .bind(changeset)
    .bind(target)
    .bind(target_draft)
    .execute(&mut *bypass)
    .await
    .expect("a pin the application couldn't make");
    bypass.commit().await.expect("commit");
    let publishing = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            commit(&mut tx, changeset, review).await
        })
    };
    until_one_waits(&owner).await;
    assert!(
        !publishing.is_finished(),
        "the release waits for the deletion"
    );
    deleting.commit().await.expect("commit");
    assert!(matches!(
        publishing.await.expect("task"),
        Err(DbError::EntryDeleted { entry: Some(entry) }) if entry == target
    ));

    // The release holds the entry first: the deletion waits, then finds it published.
    let (target, target_draft) = targets[2];
    let mut publishing = enter(&pool, alice, project).await;
    let pins = changesets::pins(&mut publishing, &[target])
        .await
        .expect("pins");
    let (changeset, review) = approved(&mut publishing, &pins).await;
    publishing.commit().await.expect("commit");
    let mut publishing = enter(&pool, alice, project).await;
    assert!(matches!(
        commit(&mut publishing, changeset, review)
            .await
            .expect("commit"),
        Committed::Done { .. }
    ));
    let deleting = {
        let pool = pool.clone();
        tokio::spawn(async move {
            let mut tx = enter(&pool, alice, project).await;
            entries::delete(&mut tx, ARTICLES, target, target_draft).await
        })
    };
    until_one_waits(&owner).await;
    assert!(
        !deleting.is_finished(),
        "the deletion waits for the release"
    );
    publishing.commit().await.expect("commit");
    assert_eq!(
        deleting.await.expect("task").expect("delete"),
        Deleted::Published
    );
    owner.close().await;
    pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_deletion_works_only_inside_its_own_project() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let other = projects::create(&db.app, alice, "beta", "Beta")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, draft) = create(&mut tx, ARTICLES, &Draft::new("Alpha's"))
        .await
        .expect("created");
    tx.commit().await.expect("commit");

    // From another project's context, the function refuses to run for this one...
    let mut tx = enter(&db.app, alice, other).await;
    let refused =
        sqlx::query("SELECT * FROM colophon_delete_entry($1, $2, $3, 'articles', $4, $5)")
            .bind(project)
            .bind(Uuid::now_v7())
            .bind(entry)
            .bind(draft)
            .bind(Uuid::now_v7())
            .fetch_one(&mut *tx.transaction)
            .await
            .expect_err("refused outside its project");
    assert_eq!(
        refused
            .as_database_error()
            .and_then(sqlx::error::DatabaseError::code)
            .as_deref(),
        Some("42501"),
        "{refused}"
    );
    drop(tx);
    // ...and in its own, finds nothing of the other project's.
    let mut tx = enter(&db.app, alice, other).await;
    assert_eq!(
        entries::delete(&mut tx, ARTICLES, entry, draft)
            .await
            .expect("delete"),
        Deleted::NotFound
    );
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    assert!(
        entries::get(&mut tx, ARTICLES, entry)
            .await
            .expect("get")
            .is_some()
    );
    drop(tx);
    db.drop().await;
}
