//! Change hints on the `colophon_events` channel (D-108, migration 0044), against real
//! PostgreSQL: a committed write notifies, a rolled-back one never does, identical payloads of
//! one transaction fold into one notification, and the wire format decodes into `Hint`. Needs
//! the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    PgListener, PgPool, destinations,
    entries::{self, NewRevision},
    events::{CHANNEL, Hint, Kind},
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
        }
    }
}

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member, in an environment that exists")
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
    let hint = next_hint(&mut listener).await;
    assert_eq!(
        (hint.project, hint.environment, hint.kind),
        (project, Some(environment), Kind::Destinations),
        "the rolled-back entry said nothing: {hint:?}"
    );
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
