//! Slugs and content-type changes against real PostgreSQL (D-054, D-055): two drafts of a
//! collection never share a slug, a release never publishes a slug another entry still publishes,
//! the draft copy of a slug always matches the draft revision, a content-type change waits for the
//! writes under way, and the stored-content check sees every draft and publication. Needs the
//! local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    DbError, PgPool, changesets, content_types,
    entries::{self, NewRevision, Saved},
    projects,
    releases::{self, Committed},
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);
const PAGES: &str = r#"{"name":"Pages","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false},{"id":"slug","label":"URL name","type":"slug","required":false}]}"#;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// A revision with this title and slug. The database layer stores what the API validated; these
/// tests only need the columns the slug rules read.
fn revision<'a>(title: &'a str, slug: Option<&'a str>) -> NewRevision<'a> {
    NewRevision {
        id: Uuid::now_v7(),
        content: r#"{"title":"Slug test"}"#,
        content_hash: &[7; 32],
        title,
        slug,
        schema_version: 1,
    }
}

async fn create(
    tx: &mut ProjectTransaction,
    collection: &str,
    title: &str,
    slug: Option<&str>,
) -> Result<(Uuid, Uuid), DbError> {
    let entry = Uuid::now_v7();
    let new = revision(title, slug);
    entries::create(tx, entry, collection, new).await?;
    Ok((entry, new.id))
}

/// Saves a new draft of `entry` on top of `base`; the new revision's id.
async fn save(
    tx: &mut ProjectTransaction,
    entry: Uuid,
    base: Uuid,
    slug: Option<&str>,
) -> Result<Uuid, DbError> {
    let new = revision("Saved", slug);
    match entries::save(tx, entry, base, new).await? {
        Saved::Saved(_) => Ok(new.id),
        other => panic!("expected a save, got {other:?}"),
    }
}

/// Pins `entries` into an approved changeset and commits it as the next release.
async fn publish(tx: &mut ProjectTransaction, entries: &[Uuid]) -> Result<Committed, DbError> {
    let pins = changesets::pins(tx, entries).await?;
    let changeset = Uuid::now_v7();
    changesets::create(tx, changeset, "Slugs", &pins, &[9; 32]).await?;
    changesets::submit(tx, changeset).await?;
    changesets::lock(tx, changeset).await?;
    changesets::review(tx, changeset, &[9; 32], "approve", "").await?;
    let review = changesets::reviews(tx, changeset, None).await?[0].id;
    let (_, base) = releases::generation(tx).await?;
    releases::commit(tx, base, changeset, &[9; 32], review, HOUR).await
}

async fn alpha(db: &TestDb) -> (Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    (alice, project)
}

/// Every entry's `draft_slug` is its draft revision's slug (migration 0015's promise, which no
/// foreign key enforces).
async fn assert_draft_slugs_match(db: &TestDb) {
    let owner = db.owner().await;
    let (mismatched,): (i64,) = sqlx::query_as(
        "SELECT count(*) FROM entries e
         JOIN revisions r ON r.project_id = e.project_id AND r.environment_id = e.environment_id
                         AND r.id = e.draft_revision_id
         WHERE e.draft_slug IS DISTINCT FROM r.slug",
    )
    .fetch_one(&owner)
    .await
    .expect("count");
    assert_eq!(mismatched, 0, "draft_slug always copies the draft's slug");
    owner.close().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn two_drafts_of_a_collection_never_share_a_slug() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let (first, first_draft) = create(&mut tx, "articles", "First", Some("intro"))
        .await
        .expect("created");
    tx.commit().await.expect("commit");

    // A second draft can't take it; the refusal rolls the whole write back.
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        create(&mut tx, "articles", "Second", Some("intro")).await,
        Err(DbError::SlugTaken)
    ));
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    let (second, second_draft) = create(&mut tx, "articles", "Second", Some("other"))
        .await
        .expect("another slug is free");
    // Entries without a slug never collide.
    create(&mut tx, "articles", "Third", None)
        .await
        .expect("no slug");
    create(&mut tx, "articles", "Fourth", None)
        .await
        .expect("no slug either");
    tx.commit().await.expect("commit");

    // Once the first draft moves to a new slug, its old one is free.
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        save(&mut tx, second, second_draft, Some("intro")).await,
        Err(DbError::SlugTaken)
    ));
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    save(&mut tx, first, first_draft, Some("renamed"))
        .await
        .expect("renamed");
    save(&mut tx, second, second_draft, Some("intro"))
        .await
        .expect("taken over");
    tx.commit().await.expect("commit");

    // Another collection has its own slugs.
    let mut tx = enter(&db.app, alice, project).await;
    content_types::create(&mut tx, "pages", PAGES, &[1; 32])
        .await
        .expect("pages");
    create(&mut tx, "pages", "Intro page", Some("intro"))
        .await
        .expect("the same slug in another collection");
    tx.commit().await.expect("commit");

    assert_draft_slugs_match(&db).await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_holder_of_a_slug_is_named_but_never_the_entry_itself() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let (entry, draft) = create(&mut tx, "articles", "Hello", Some("hello"))
        .await
        .expect("created");
    let holder = entries::slug_holder(&mut tx, "articles", "hello", None)
        .await
        .expect("holder")
        .expect("someone has it");
    assert_eq!(
        (holder.entry_id, holder.title.as_str(), holder.published),
        (entry, "Hello", false)
    );
    assert_eq!(
        entries::slug_holder(&mut tx, "articles", "hello", Some(entry))
            .await
            .expect("holder"),
        None,
        "an entry may keep its own slug"
    );
    assert!(matches!(
        publish(&mut tx, &[entry]).await.expect("published"),
        Committed::Done { .. }
    ));
    // Renamed in its draft, the published slug still belongs to it.
    save(&mut tx, entry, draft, Some("hello-again"))
        .await
        .expect("renamed");
    let holder = entries::slug_holder(&mut tx, "articles", "hello", None)
        .await
        .expect("holder")
        .expect("still published");
    assert_eq!((holder.entry_id, holder.published), (entry, true));
    tx.commit().await.expect("commit");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_release_never_publishes_a_slug_another_entry_still_publishes() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let (first, first_draft) = create(&mut tx, "articles", "First", Some("intro"))
        .await
        .expect("created");
    publish(&mut tx, &[first]).await.expect("published");
    // The first entry's draft moves on, freeing "intro" among drafts but not among what is
    // published; the database lets a second draft take it.
    save(&mut tx, first, first_draft, Some("intro-moved"))
        .await
        .expect("renamed");
    let (second, _) = create(&mut tx, "articles", "Second", Some("intro"))
        .await
        .expect("drafts may take it");
    tx.commit().await.expect("commit");

    // Publishing the second alone would put "intro" live twice: refused, nothing committed.
    let mut tx = enter(&db.app, alice, project).await;
    assert!(matches!(
        publish(&mut tx, &[second]).await,
        Err(DbError::SlugTaken)
    ));
    drop(tx);
    let mut tx = enter(&db.app, alice, project).await;
    assert_eq!(
        releases::generation(&mut tx).await.expect("generation").1,
        1,
        "the refused release left no trace"
    );
    // Both together: the first moves to its new slug in the same release.
    assert!(matches!(
        publish(&mut tx, &[first, second]).await.expect("published"),
        Committed::Done { generation: 2, .. }
    ));
    tx.commit().await.expect("commit");
    assert_draft_slugs_match(&db).await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_content_type_change_waits_for_the_writes_under_way() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut writer = enter(&db.app, alice, project).await;
    content_types::lock_for_write(&mut writer, "articles")
        .await
        .expect("shared");
    // A second writer shares the lock.
    let mut other = enter(&db.app, alice, project).await;
    tokio::time::timeout(
        Duration::from_secs(2),
        content_types::lock_for_write(&mut other, "articles"),
    )
    .await
    .expect("writers don't wait for each other")
    .expect("shared");
    drop(other);

    let pool = db.app.clone();
    let change = tokio::spawn(async move {
        let mut tx = enter(&pool, alice, project).await;
        content_types::lock_for_change(&mut tx, "articles")
            .await
            .expect("exclusive");
    });
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(!change.is_finished(), "the change waits for the writer");
    writer.commit().await.expect("commit");
    tokio::time::timeout(Duration::from_secs(5), change)
        .await
        .expect("the change goes ahead once the writer is done")
        .expect("joined");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_change_checks_every_draft_and_every_publication_that_isn_t_a_draft() {
    let db = TestDb::new().await;
    let (alice, project) = alpha(&db).await;
    let mut tx = enter(&db.app, alice, project).await;
    let mut made = Vec::new();
    for index in 0..5 {
        let title = format!("Entry {index}");
        made.push(create(&mut tx, "articles", &title, None).await.expect("created"));
    }
    // Two published; one of them then gets a new draft, so its publication is a second revision.
    let ids: Vec<Uuid> = made.iter().map(|(entry, _)| *entry).collect();
    publish(&mut tx, &ids[..2]).await.expect("published");
    save(&mut tx, made[0].0, made[0].1, None)
        .await
        .expect("saved");
    assert_eq!(
        content_types::stored_count(&mut tx, "articles")
            .await
            .expect("count"),
        6,
        "five drafts and the one publication that is no longer the draft"
    );
    let mut seen = Vec::new();
    for published in [false, true] {
        let mut after = Uuid::nil();
        loop {
            let page = content_types::stored_page(&mut tx, "articles", published, after, 2)
                .await
                .expect("page");
            let Some(last) = page.last() else { break };
            after = last.entry_id;
            assert!(page.iter().all(|stored| stored.published == published));
            seen.extend(page.into_iter().map(|stored| stored.revision_id));
        }
    }
    assert_eq!(seen.len(), 6);
    assert!(seen.contains(&made[0].1), "the published, superseded revision");
    tx.commit().await.expect("commit");
    db.drop().await;
}
