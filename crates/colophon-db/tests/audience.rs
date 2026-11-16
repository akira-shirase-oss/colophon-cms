//! The audience against real PostgreSQL (D-137): signups take only open roles, sessions live
//! and die by their own clocks, a disable or a revocation bites at once, and — the point of
//! the design — `colophon_delivery`'s row-level security itself hides gated publications,
//! so nothing upstream of it decides who may read. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    PgPool, audience, changesets,
    delivery::{self, Audience},
    entries::{self, NewRevision},
    passwords, projects, releases,
    releases::Committed,
    scope::{self, MemberTransaction, ProjectTransaction},
};
use common::TestDb;
use sqlx::Row;
use uuid::Uuid;

const HOUR: std::time::Duration = std::time::Duration::from_secs(3600);

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

async fn staff(pool: &PgPool, identity: Uuid, project: Uuid) -> MemberTransaction {
    scope::begin_as_member(pool, identity, project)
        .await
        .expect("begin")
        .expect("a member")
}

/// A project with an `owner` member and the audience roles `member` (open to signup) and
/// `vip` (staff only).
async fn project(db: &TestDb) -> (Uuid, Uuid) {
    let owner = db.administrator("owner").await;
    let project = projects::create(&db.app, owner, "site", "Site")
        .await
        .expect("created")
        .id;
    let mut tx = staff(&db.app, owner, project).await;
    assert!(
        audience::create_role(&mut tx, "member", true)
            .await
            .expect("role")
    );
    assert!(
        audience::create_role(&mut tx, "vip", false)
            .await
            .expect("role")
    );
    tx.commit().await.expect("commit");
    (owner, project)
}

/// An entry published and released, gated as `visibility`; returns (entry, generation).
async fn publish_generation(
    db: &TestDb,
    owner: Uuid,
    project: Uuid,
    title: &str,
    visibility: &str,
) -> (Uuid, i64) {
    let mut tx = enter(&db.app, owner, project).await;
    let (entry, revision) = (Uuid::now_v7(), Uuid::now_v7());
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: &format!(r#"{{"title":"{title}"}}"#),
            content_hash: &[3; 32],
            title,
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: Some(visibility),
        },
        None,
    )
    .await
    .expect("created");
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, title, &pins, &[5; 32], None)
        .await
        .expect("created");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[5; 32], "approve", "", None)
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    let base = releases::generation(&mut tx).await.expect("generation").1;
    let Committed::Done { generation, .. } =
        releases::commit(&mut tx, base, changeset, &[5; 32], review, HOUR)
            .await
            .expect("commit")
    else {
        panic!("the release commits");
    };
    tx.commit().await.expect("commit");
    (entry, generation)
}

/// The titles a delivery read of `articles` shows `audience` at `generation`.
async fn seen(db: &TestDb, project: Uuid, audience: &Audience, generation: i64) -> Vec<String> {
    let mut tx = delivery::begin(&db.app, project, audience)
        .await
        .expect("begin");
    let environment = tx
        .environment("production")
        .await
        .expect("environment")
        .expect("found");
    let rows = tx
        .list(environment.id, "articles", generation, None, 20)
        .await
        .expect("list");
    tx.finish().await.expect("finish");
    let mut titles: Vec<_> = rows.into_iter().map(|row| row.title).collect();
    titles.sort();
    titles
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn signup_takes_only_open_roles_and_staff_can_give_closed_ones() {
    let db = TestDb::new().await;
    let (owner, project) = project(&db).await;
    let phc = passwords::hashed("correct horse battery")
        .await
        .expect("hash");

    // Two open roles would make a role-less signup ambiguous; `member` is the only one.
    assert!(matches!(
        audience::sign_up(&db.app, project, "a@b.c", &phc, None, None)
            .await
            .expect("sign_up"),
        audience::SignedUp::Made(_)
    ));
    // An explicit open role works.
    assert!(matches!(
        audience::sign_up(&db.app, project, "b@b.c", &phc, None, Some("member"))
            .await
            .expect("sign_up"),
        audience::SignedUp::Made(_)
    ));
    // A closed role is refused to signup — and so is a role that doesn't exist.
    for role in ["vip", "ghost"] {
        assert_eq!(
            audience::sign_up(&db.app, project, "c@b.c", &phc, None, Some(role))
                .await
                .expect("sign_up"),
            audience::SignedUp::RoleUnavailable,
            "{role}"
        );
    }
    // The email is taken.
    assert_eq!(
        audience::sign_up(&db.app, project, "a@b.c", &phc, None, None)
            .await
            .expect("sign_up"),
        audience::SignedUp::Taken
    );
    // Staff put a user in the closed `vip` role.
    let mut tx = staff(&db.app, owner, project).await;
    let vip = match audience::create_user(&mut tx, "d@b.c", &phc, None, "vip")
        .await
        .expect("create_user")
    {
        audience::SignedUp::Made(user) => user,
        other => panic!("staff can assign a closed role: {other:?}"),
    };
    assert_eq!(vip.role, "vip");
    // A role users hold can't be deleted.
    assert!(matches!(
        audience::delete_role(&mut tx, "vip").await.expect("delete"),
        audience::RoleGone::InUse(1)
    ));
    // Move the user off it and it deletes.
    assert!(matches!(
        audience::update_user(&mut tx, vip.id, Some("member"), None)
            .await
            .expect("update"),
        audience::UserChanged::Updated(_)
    ));
    assert_eq!(
        audience::delete_role(&mut tx, "vip").await.expect("delete"),
        audience::RoleGone::Deleted
    );
    // And the unknown-role change is refused, leaving the user's role alone.
    assert_eq!(
        audience::update_user(&mut tx, vip.id, Some("vip"), None)
            .await
            .expect("update"),
        audience::UserChanged::UnknownRole
    );
    tx.commit().await.expect("commit");
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn delivery_row_security_gates_published_content_by_audience() {
    let db = TestDb::new().await;
    let (owner, project) = project(&db).await;
    publish_generation(&db, owner, project, "For everyone", "public").await;
    publish_generation(&db, owner, project, "For accounts", "authenticated").await;
    publish_generation(&db, owner, project, "For members", "role:member").await;
    // Reads at the last generation see every publication still open there.
    let (_, generation) = publish_generation(&db, owner, project, "For vips", "role:vip").await;

    // Anonymous reads public alone — at the database, not a filter over it.
    assert_eq!(
        seen(&db, project, &Audience::Anonymous, generation)
            .await
            .as_slice(),
        ["For everyone"]
    );
    // Any live session reads `authenticated`; each role reads its own gate.
    assert_eq!(
        seen(
            &db,
            project,
            &Audience::Member {
                role: "member".to_owned()
            },
            generation
        )
        .await
        .as_slice(),
        ["For accounts", "For everyone", "For members"]
    );
    assert_eq!(
        seen(
            &db,
            project,
            &Audience::Member {
                role: "vip".to_owned()
            },
            generation
        )
        .await
        .as_slice(),
        ["For accounts", "For everyone", "For vips"]
    );
    // A role nobody gates to reads the first two alone.
    assert_eq!(
        seen(
            &db,
            project,
            &Audience::Member {
                role: "ghost".to_owned()
            },
            generation
        )
        .await
        .as_slice(),
        ["For accounts", "For everyone"]
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn sessions_live_and_die_by_expiry_idle_revocation_and_disable() {
    let db = TestDb::new().await;
    let (owner_id, project) = project(&db).await;
    let phc = passwords::hashed("correct horse battery")
        .await
        .expect("hash");
    let audience::SignedUp::Made(user) =
        audience::sign_up(&db.app, project, "a@b.c", &phc, None, None)
            .await
            .expect("sign_up")
    else {
        panic!("signed up");
    };
    let hash = [9; 32];
    let session = audience::create_session(&db.app, project, user.id, &hash, &[8; 32])
        .await
        .expect("session");
    let found = audience::find_session(&db.app, project, &hash)
        .await
        .expect("find")
        .expect("there");
    assert_eq!(found.session_id, session);
    assert_eq!(found.role, "member");
    assert!(found.live());

    // Revoked is dead at once, and stays recorded.
    audience::revoke_session(&db.app, project, session)
        .await
        .expect("revoke");
    let found = audience::find_session(&db.app, project, &hash)
        .await
        .expect("find")
        .expect("still a row");
    assert!(!found.live());

    // An expired session is dead too: age one past its end as the owner.
    let session = audience::create_session(&db.app, project, user.id, &[7; 32], &[8; 32])
        .await
        .expect("session");
    let owner = db.owner().await;
    // `expires_at > created_at` is a CHECK, so aging the end means aging the start too.
    sqlx::query(
        "UPDATE site_sessions
         SET created_at = now() - interval '31 days', expires_at = now() - interval '1 day'
         WHERE id = $1",
    )
    .bind(session)
    .execute(&owner)
    .await
    .expect("aged");
    let found = audience::find_session(&db.app, project, &[7; 32])
        .await
        .expect("find")
        .expect("there");
    assert!(!found.live(), "expired");

    // And one idle past IDLE: back-date its last touch.
    let session = audience::create_session(&db.app, project, user.id, &[6; 32], &[8; 32])
        .await
        .expect("session");
    sqlx::query("UPDATE site_sessions SET last_seen_at = now() - interval '8 days' WHERE id = $1")
        .bind(session)
        .execute(&owner)
        .await
        .expect("idled");
    let found = audience::find_session(&db.app, project, &[6; 32])
        .await
        .expect("find")
        .expect("there");
    assert!(!found.live(), "idle");
    owner.close().await;

    // A disabled account's sessions are revoked and it can't sign in. The live session is
    // made before the staff transaction opens — the pool holds a single connection.
    let live = audience::create_session(&db.app, project, user.id, &[5; 32], &[8; 32])
        .await
        .expect("session");
    let mut tx = staff(&db.app, owner_id, project).await;
    assert!(matches!(
        audience::update_user(&mut tx, user.id, None, Some(true))
            .await
            .expect("disable"),
        audience::UserChanged::Updated(_)
    ));
    tx.commit().await.expect("commit");
    let found = audience::find_session(&db.app, project, &[5; 32])
        .await
        .expect("find")
        .expect("there");
    assert!(
        !found.live(),
        "disabled: the just-made session {live} is dead"
    );
    assert_eq!(
        audience::sign_in(
            &db.app,
            &common::gate(),
            project,
            "a@b.c",
            "203.0.113.1",
            "correct horse battery"
        )
        .await
        .expect("sign_in"),
        audience::SignIn::Disabled
    );
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn signin_refuses_wrong_passwords_and_locks_after_enough() {
    let db = TestDb::new().await;
    let (_owner, project) = project(&db).await;
    let phc = passwords::hashed("correct horse battery")
        .await
        .expect("hash");
    assert!(matches!(
        audience::sign_up(&db.app, project, "a@b.c", &phc, None, None)
            .await
            .expect("sign_up"),
        audience::SignedUp::Made(_)
    ));

    // A wrong password and no account answer alike, and a few wrongs don't lock.
    assert_eq!(
        audience::sign_in(
            &db.app,
            &common::gate(),
            project,
            "a@b.c",
            "203.0.113.1",
            "wrong password"
        )
        .await
        .expect("sign_in"),
        audience::SignIn::Refused
    );
    assert_eq!(
        audience::sign_in(
            &db.app,
            &common::gate(),
            project,
            "nobody@b.c",
            "203.0.113.1",
            "wrong password"
        )
        .await
        .expect("sign_in"),
        audience::SignIn::Refused
    );
    // LOCK_AFTER bad tries pause that address's key — even the right password is refused
    // there, exactly as a wrong one (D-154) — while another address signs in.
    for _ in 1..audience::LOCK_AFTER {
        assert_eq!(
            audience::sign_in(
                &db.app,
                &common::gate(),
                project,
                "a@b.c",
                "203.0.113.1",
                "wrong password"
            )
            .await
            .expect("sign_in"),
            audience::SignIn::Refused
        );
    }
    assert_eq!(
        audience::sign_in(
            &db.app,
            &common::gate(),
            project,
            "a@b.c",
            "203.0.113.1",
            "correct horse battery"
        )
        .await
        .expect("sign_in"),
        audience::SignIn::Refused
    );
    assert!(matches!(
        audience::sign_in(
            &db.app,
            &common::gate(),
            project,
            "a@b.c",
            "203.0.113.2",
            "correct horse battery"
        )
        .await
        .expect("sign_in"),
        audience::SignIn::SignedIn(_)
    ));
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[expect(
    clippy::too_many_lines,
    reason = "the link's whole lifecycle in one test: listed, live, spent, retired, expired, scoped"
)]
async fn magic_links_are_single_use_superseded_expiring_and_project_scoped() {
    let db = TestDb::new().await;
    let (owner, project) = project(&db).await;
    let phc = passwords::hashed("correct horse battery")
        .await
        .expect("hash");
    let audience::SignedUp::Made(user) =
        audience::sign_up(&db.app, project, "a@b.c", &phc, None, None)
            .await
            .expect("sign_up")
    else {
        panic!("signed up");
    };
    let return_to = "http://site.test/welcome";
    let origin = "http://site.test";

    // Nothing listed: no link is recorded for any origin (D-148).
    assert_eq!(
        audience::request_magic_link(&db.app, project, "a@b.c", &[9; 32], return_to, origin)
            .await
            .expect("request"),
        audience::LinkRequest::OriginNotAllowed
    );
    let mut tx = staff(&db.app, owner, project).await;
    audience::set_origins(&mut tx, &[origin.to_owned()])
        .await
        .expect("listed");
    assert_eq!(audience::origins(&mut tx).await.expect("origins"), [origin]);
    tx.commit().await.expect("commit");
    assert_eq!(
        audience::consume_magic_link(&db.app, project, &[9; 32])
            .await
            .expect("consume"),
        audience::MagicLink::Invalid,
        "the refused request stored nothing"
    );

    // A link for an existing account signs it in — the answer names the user.
    let first = [1; 32];
    audience::request_magic_link(&db.app, project, "a@b.c", &first, return_to, origin)
        .await
        .expect("request");
    match audience::consume_magic_link(&db.app, project, &first)
        .await
        .expect("consume")
    {
        audience::MagicLink::SignedIn(found) => {
            assert_eq!(found.id, user.id);
            assert_eq!(found.email, "a@b.c");
        }
        other => panic!("a live link signs its account in: {other:?}"),
    }
    // And only once.
    assert_eq!(
        audience::consume_magic_link(&db.app, project, &first)
            .await
            .expect("consume"),
        audience::MagicLink::Invalid,
        "a used link is dead"
    );

    // An unknown email's link is the signup half: NewUser, still single-use.
    let second = [2; 32];
    audience::request_magic_link(&db.app, project, "new@b.c", &second, return_to, origin)
        .await
        .expect("request");
    assert_eq!(
        audience::consume_magic_link(&db.app, project, &second)
            .await
            .expect("consume"),
        audience::MagicLink::NewUser("new@b.c".to_owned())
    );
    assert_eq!(
        audience::consume_magic_link(&db.app, project, &second)
            .await
            .expect("consume"),
        audience::MagicLink::Invalid
    );

    // Asking again never retires a link someone holds (D-148): both stay live until one is
    // spent, and spending either retires the other.
    let (held, asked) = ([3; 32], [4; 32]);
    let owner_pool = db.owner().await;
    for token in [&held, &asked] {
        assert_eq!(
            audience::request_magic_link(&db.app, project, "a@b.c", token, return_to, origin)
                .await
                .expect("request"),
            audience::LinkRequest::Recorded
        );
        // Past the address's cooldown (D-153) before the next request.
        sqlx::query(
            "UPDATE audience_magic_links SET created_at = created_at - interval '2 minutes'
             WHERE token_hash = $1",
        )
        .bind(&token[..])
        .execute(&owner_pool)
        .await
        .expect("aged");
    }
    owner_pool.close().await;
    assert!(matches!(
        audience::consume_magic_link(&db.app, project, &held)
            .await
            .expect("consume"),
        audience::MagicLink::SignedIn(_)
    ));
    assert_eq!(
        audience::consume_magic_link(&db.app, project, &asked)
            .await
            .expect("consume"),
        audience::MagicLink::Invalid,
        "spending one link retires its siblings"
    );

    // An expired link is dead too: `expires_at > created_at` is a CHECK, so age both as the
    // owner (the application login can't write the table's timestamps — and RLS hides the
    // rows from it outside a project scope anyway).
    let expired = [5; 32];
    audience::request_magic_link(&db.app, project, "a@b.c", &expired, return_to, origin)
        .await
        .expect("request");
    let owner_pool = db.owner().await;
    sqlx::query(
        "UPDATE audience_magic_links
         SET created_at = now() - interval '20 minutes', expires_at = now() - interval '5 minutes'
         WHERE token_hash = $1",
    )
    .bind(&expired[..])
    .execute(&owner_pool)
    .await
    .expect("aged");
    assert_eq!(
        audience::consume_magic_link(&db.app, project, &expired)
            .await
            .expect("consume"),
        audience::MagicLink::Invalid,
        "an expired link is dead"
    );

    // Another project's scope sees none of this project's links: the same token hash is
    // Invalid there, and the application's login can't even read the rows across.
    let other = projects::create(&db.app, owner, "other", "Other")
        .await
        .expect("created")
        .id;
    let sixth = [6; 32];
    audience::request_magic_link(&db.app, project, "a@b.c", &sixth, return_to, origin)
        .await
        .expect("request");
    assert_eq!(
        audience::consume_magic_link(&db.app, other, &sixth)
            .await
            .expect("consume"),
        audience::MagicLink::Invalid,
        "a link names its own project only"
    );
    let visible = sqlx::query("SELECT count(*) AS n FROM audience_magic_links")
        .fetch_one(&db.app)
        .await
        .expect("count")
        .get::<i64, _>("n");
    assert_eq!(visible, 0, "the app login holds no cross-project read");
    owner_pool.close().await;
    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_session_beyond_the_bound_pushes_the_oldest_out() {
    let db = TestDb::new().await;
    let (_owner, project) = project(&db).await;
    let phc = passwords::hashed("correct horse battery")
        .await
        .expect("hash");
    let audience::SignedUp::Made(user) =
        audience::sign_up(&db.app, project, "a@b.c", &phc, None, None)
            .await
            .expect("sign_up")
    else {
        panic!("signed up");
    };
    // MAX_SESSIONS + 1 sessions: the first is revoked to make room.
    let mut hashes = Vec::new();
    for index in 0..=audience::MAX_SESSIONS {
        let mut hash = [0; 32];
        hash[0] = u8::try_from(index).unwrap_or(0);
        audience::create_session(&db.app, project, user.id, &hash, &[8; 32])
            .await
            .expect("session");
        hashes.push(hash);
    }
    let first = audience::find_session(&db.app, project, &hashes[0])
        .await
        .expect("find")
        .expect("there");
    assert!(
        !first.live(),
        "the oldest of {} was pushed out",
        audience::MAX_SESSIONS + 1
    );
    // Counted as the owner: `site_sessions` is row-secured, so `colophon_app` sees none
    // without a project scope — which is itself part of the point.
    let owner = db.owner().await;
    let live = sqlx::query(
        "SELECT count(*) AS n FROM site_sessions WHERE site_user_id = $1 AND revoked_at IS NULL",
    )
    .bind(user.id)
    .fetch_one(&owner)
    .await
    .expect("count")
    .get::<i64, _>("n");
    assert_eq!(live, audience::MAX_SESSIONS);
    owner.close().await;
    db.drop().await;
}
