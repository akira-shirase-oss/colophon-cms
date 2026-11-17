//! Change hints on the `colophon_events` channel (D-108, migrations 0044 and 0068), against
//! real PostgreSQL: a committed write queues its hint and the pump sends it once, a writer
//! never notifies by itself, a rolled-back write sends nothing, identical hints fold into one
//! notification however many rows or statements queued them, and the wire format decodes into
//! `Hint`. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    PgListener, PgPool, destinations,
    entries::{self, NewRevision},
    events::{self, CHANNEL, Hint, Kind},
    projects,
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use uuid::Uuid;

const ARTICLES: &str = "articles";
/// Long enough for Postgres to dispatch a commit's notifications.
const RECEIVE: Duration = Duration::from_secs(5);
/// Short enough to keep the suite quick, long enough that a notification can't still be "on
/// the way" after a commit returned.
const QUIET: Duration = Duration::from_millis(500);

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
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
        }
    }
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member, in an environment that exists")
}

/// Runs one pump pass, as a server's pump does every 100 ms (D-180): what writers queued is
/// sent now.
async fn pump(pool: &PgPool) -> u64 {
    events::pump(pool).await.expect("the pump runs")
}

/// The next decoded hint; fails if none arrives or the payload doesn't parse.
async fn next_hint(listener: &mut PgListener) -> Hint {
    let notification = tokio::time::timeout(RECEIVE, listener.try_recv())
        .await
        .expect("a notification arrives")
        .expect("the listener stays connected")
        .expect("a notification, not a reconnect");
    Hint::decode(notification.payload()).expect("the payload decodes")
}

/// Asserts nothing arrives within `QUIET`.
async fn quiet(listener: &mut PgListener) {
    if let Ok(got) = tokio::time::timeout(QUIET, listener.try_recv()).await {
        let detail = match got {
            Ok(Some(notification)) => notification.payload().to_owned(),
            Ok(None) => "a reconnect".to_owned(),
            Err(error) => format!("listener error: {error}"),
        };
        panic!("an unexpected notification arrived: {detail}");
    }
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_commit_notifies_and_a_rollback_does_not() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    // The project's own hints were queued before anyone listened: send them now.
    pump(&db.app).await;

    let mut listener = db.listener().await;
    listener.listen(CHANNEL).await.expect("subscribed");

    // A committed entry notifies once, naming its project, environment and collection.
    let mut tx = enter(&db.app, alice, project).await;
    let environment = tx.environment;
    let first = Content::new("First");
    entries::create(&mut tx, Uuid::now_v7(), ARTICLES, first.revision(), None)
        .await
        .expect("entry");
    tx.commit().await.expect("commit");
    // The writer queued its hint and notified nothing itself (D-180): the commit never took
    // PostgreSQL's notify lock. The pump sends it.
    quiet(&mut listener).await;
    assert!(pump(&db.app).await >= 1);
    assert_eq!(
        next_hint(&mut listener).await,
        Hint {
            project,
            environment: Some(environment),
            kind: Kind::Entries,
            collection: Some(ARTICLES.to_owned()),
        }
    );

    // A rolled-back write notifies nothing: the next commit's hint is the next to arrive.
    let mut tx = enter(&db.app, alice, project).await;
    let second = Content::new("Second");
    entries::create(&mut tx, Uuid::now_v7(), ARTICLES, second.revision(), None)
        .await
        .expect("entry");
    tx.transaction.rollback().await.expect("rollback");
    let mut tx = enter(&db.app, alice, project).await;
    destinations::create(
        &mut tx,
        "webhook",
        "hook",
        "https://hooks.example.com/0",
        None,
    )
    .await
    .expect("destination");
    tx.commit().await.expect("commit");
    pump(&db.app).await;
    let hint = next_hint(&mut listener).await;
    assert_eq!(
        (hint.project, hint.environment, hint.kind),
        (project, Some(environment), Kind::Destinations),
        "the rolled-back entry said nothing: {hint:?}"
    );
    // Nothing is left to send: a second pass is empty.
    assert_eq!(pump(&db.app).await, 0);
    quiet(&mut listener).await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn one_transaction_folds_identical_hints() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    // The project's own hints were queued before anyone listened: send them now.
    pump(&db.app).await;

    let mut listener = db.listener().await;
    listener.listen(CHANNEL).await.expect("subscribed");

    // Two entries of one collection in one commit: PostgreSQL sends the identical payload once.
    let mut tx = enter(&db.app, alice, project).await;
    let environment = tx.environment;
    for title in ["One", "Two"] {
        let content = Content::new(title);
        entries::create(&mut tx, Uuid::now_v7(), ARTICLES, content.revision(), None)
            .await
            .expect("entry");
    }
    tx.commit().await.expect("commit");
    pump(&db.app).await;
    assert_eq!(
        next_hint(&mut listener).await,
        Hint {
            project,
            environment: Some(environment),
            kind: Kind::Entries,
            collection: Some(ARTICLES.to_owned()),
        }
    );
    quiet(&mut listener).await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn creating_a_project_hints_types_roles_and_members() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let mut listener = db.listener().await;
    listener.listen(CHANNEL).await.expect("subscribed");

    // One commit inserts the articles collection and its content type (types), the built-in
    // roles and their permissions (roles), and the owner's membership (members). The types'
    // two writes share one payload and fold; the project-level rows carry no environment.
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let environment = {
        let tx = enter(&db.app, alice, project).await;
        let id = tx.environment;
        tx.commit().await.expect("commit");
        id
    };
    pump(&db.app).await;
    for expected in [
        (Some(environment), Kind::Types),
        (None, Kind::Roles),
        (None, Kind::Members),
    ] {
        let hint = next_hint(&mut listener).await;
        assert_eq!(
            (hint.project, hint.environment, hint.kind),
            (project, expected.0, expected.1),
            "{hint:?}"
        );
    }
    quiet(&mut listener).await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_thousand_row_write_queues_one_hint_and_sends_one_notification() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    pump(&db.app).await;
    let mut listener = db.listener().await;
    listener.listen(CHANNEL).await.expect("subscribed");

    // One statement, 1,000 audit events: one queued hint, not 1,000 (the statement-level
    // trigger writes each distinct hint once).
    let tx = enter(&db.app, alice, project).await;
    let mut transaction = tx.transaction;
    sqlx::query(
        "INSERT INTO audit_events (project_id, id, actor_id, action, subject, detail)
         SELECT $1, gen_random_uuid(), $2, 'test.bulk', 'row ' || n, '{}'::jsonb
         FROM generate_series(1, 1000) AS n",
    )
    .bind(project)
    .bind(alice)
    .execute(&mut *transaction)
    .await
    .expect("1,000 rows");
    let queued: i64 = sqlx::query_scalar("SELECT count(*) FROM change_hints")
        .fetch_one(&mut *transaction)
        .await
        .expect("count");
    assert_eq!(queued, 1, "one hint for the statement");
    transaction.commit().await.expect("commit");
    quiet(&mut listener).await;
    assert_eq!(pump(&db.app).await, 1);
    let hint = next_hint(&mut listener).await;
    assert_eq!(
        (hint.project, hint.environment, hint.kind),
        (project, None, Kind::Audit)
    );
    quiet(&mut listener).await;

    // A rolled-back write leaves nothing queued.
    let tx = enter(&db.app, alice, project).await;
    let mut transaction = tx.transaction;
    sqlx::query(
        "INSERT INTO audit_events (project_id, id, actor_id, action, subject, detail)
         VALUES ($1, gen_random_uuid(), $2, 'test.rolled', 'back', '{}'::jsonb)",
    )
    .bind(project)
    .bind(alice)
    .execute(&mut *transaction)
    .await
    .expect("a row");
    transaction.rollback().await.expect("rollback");
    assert_eq!(pump(&db.app).await, 0);
    quiet(&mut listener).await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn many_transactions_of_one_hint_become_one_notification_per_pass() {
    let db = TestDb::new().await;
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    pump(&db.app).await;
    let mut listener = db.listener().await;
    listener.listen(CHANNEL).await.expect("subscribed");
    // Ten separate commits of the same kind between two passes: ten queued rows, one
    // notification — the pump folds across transactions too.
    for n in 0..10 {
        let mut tx = enter(&db.app, alice, project).await;
        let content = Content::new(&format!("Entry {n}"));
        entries::create(&mut tx, Uuid::now_v7(), ARTICLES, content.revision(), None)
            .await
            .expect("entry");
        tx.commit().await.expect("commit");
    }
    assert!(pump(&db.app).await >= 10);
    let hint = next_hint(&mut listener).await;
    assert_eq!(hint.kind, Kind::Entries);
    quiet(&mut listener).await;
    db.drop().await;
}
