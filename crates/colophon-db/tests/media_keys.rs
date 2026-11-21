//! The media signing key's ids and rotation (D-233, migration 0086): a rotation makes a new current key and keeps
//! only the one it replaces (or none, retiring the previous); two at once take turns; each leaves an
//! installation-level line, never the key; and the API's login can read keys but neither rotate nor write them.
//! Needs the local stack.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "integration tests: a failed setup step is a test failure"
)]

mod common;

use common::TestDb;
use sqlx::PgPool;

// Through SQL, as the admin command and the restore stage call it: `colophon_db::delivery`'s wrappers are one
// statement each over these.
async fn ids(pool: &PgPool) -> Vec<i16> {
    sqlx::query_scalar("SELECT id FROM media_signing_keys ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .expect("the keys")
}

async fn keys(pool: &PgPool) -> Vec<Vec<u8>> {
    sqlx::query_scalar("SELECT key FROM media_signing_keys ORDER BY id DESC")
        .fetch_all(pool)
        .await
        .expect("the keys")
}

async fn rotate_media_key(pool: &PgPool, retire: bool, reason: &str) -> Result<i16, sqlx::Error> {
    sqlx::query_scalar("SELECT colophon_rotate_media_key($1, $2)")
        .bind(retire)
        .bind(reason)
        .fetch_one(pool)
        .await
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_rotation_keeps_the_key_it_replaces_and_retires_the_rest() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    // 0082's key, kept as id 1.
    assert_eq!(ids(&owner).await, [1]);
    let first = keys(&owner).await[0].clone();
    assert_eq!(first.len(), 32);

    assert_eq!(
        rotate_media_key(&owner, false, "admin")
            .await
            .expect("rotate"),
        2
    );
    assert_eq!(
        ids(&owner).await,
        [2, 1],
        "the new key first, the one it replaced kept"
    );
    let now = keys(&owner).await;
    assert_eq!(now[1], first, "the previous key is unchanged");
    assert_ne!(now[0], first, "a new key, not a copy");
    assert_eq!(now[0].len(), 32);

    rotate_media_key(&owner, false, "admin")
        .await
        .expect("rotate");
    assert_eq!(
        ids(&owner).await,
        [3, 2],
        "key 1 retired by the second rotation"
    );

    rotate_media_key(&owner, true, "restore")
        .await
        .expect("rotate");
    assert_eq!(
        ids(&owner).await,
        [4],
        "--retire-previous keeps the new key alone"
    );

    // One line each: the id, who ran it, whether it retired the previous key, and why. Never the key.
    let lines: Vec<(i16, String, bool, String)> = sqlx::query_as(
        "SELECT kid, by_role, retired_previous, reason FROM media_key_rotations ORDER BY kid",
    )
    .fetch_all(&owner)
    .await
    .expect("the rotations");
    let me: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&owner)
        .await
        .expect("who");
    assert_eq!(
        lines,
        [
            (2, me.clone(), false, "admin".to_owned()),
            (3, me.clone(), false, "admin".to_owned()),
            (4, me, true, "restore".to_owned()),
        ]
    );
    let columns: Vec<String> = sqlx::query_scalar(
        "SELECT column_name::text FROM information_schema.columns WHERE table_name = 'media_key_rotations'",
    )
    .fetch_all(&owner)
    .await
    .expect("columns");
    assert!(
        !columns
            .iter()
            .any(|column| column.contains("key") && column != "kid"),
        "{columns:?}"
    );
    // Append-only, for the owner too.
    let changed = sqlx::query("UPDATE media_key_rotations SET reason = 'admin'")
        .execute(&owner)
        .await;
    assert!(changed.is_err(), "the rotation log refused an update");
    let removed = sqlx::query("DELETE FROM media_key_rotations")
        .execute(&owner)
        .await;
    assert!(removed.is_err(), "the rotation log refused a delete");
    db.drop().await;
}

/// Two rotations at once take turns: every id is made once, the log has a line for each, and what is left is the
/// newest key and the one before it, never two keys deleted by each other or a gap.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn concurrent_rotations_take_turns() {
    let db = TestDb::new().await;
    let owner = db.owner().await;
    let runs: Vec<_> = (0..8)
        .map(|_| {
            let owner = owner.clone();
            tokio::spawn(async move { rotate_media_key(&owner, false, "admin").await })
        })
        .collect();
    let mut made = Vec::new();
    for run in runs {
        made.push(run.await.expect("joined").expect("rotated"));
    }
    made.sort_unstable();
    assert_eq!(
        made,
        (2..=9).collect::<Vec<i16>>(),
        "each id made exactly once"
    );
    assert_eq!(ids(&owner).await, [9, 8]);
    let lines: i64 = sqlx::query_scalar("SELECT count(*) FROM media_key_rotations")
        .fetch_one(&owner)
        .await
        .expect("count");
    assert_eq!(lines, 8);
    db.drop().await;
}

/// The API's login reads keys (to sign and verify) and nothing more.
#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn the_api_reads_keys_but_can_t_rotate_or_write_them() {
    let db = TestDb::new().await;
    let app = db.app_pool(1).await;
    assert_eq!(ids(&app).await, [1], "the API reads the keys");
    let rotated = rotate_media_key(&app, false, "admin").await;
    assert!(rotated.is_err(), "the API can't rotate");
    for statement in [
        "INSERT INTO media_signing_keys (id, key) VALUES (7, '\\x00')",
        "UPDATE media_signing_keys SET key = key",
        "DELETE FROM media_signing_keys",
        "INSERT INTO media_key_rotations (kid, retired_previous, reason) VALUES (7, false, 'admin')",
        "SELECT * FROM media_key_rotations",
    ] {
        let tried = sqlx::query(statement).execute(&app).await;
        assert!(tried.is_err(), "the API ran: {statement}");
    }
    db.drop().await;
}
