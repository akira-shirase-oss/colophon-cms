//! The work bounds against real PostgreSQL (spec §33, D-131, D-135): beside the per-project
//! count of open rows, the installation's pending payload refuses new durable work at a
//! gibibyte of stored metadata — counted as `colophon_pending_payload` counts it, freed as
//! open work finishes. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use colophon_db::{
    DbError, PgPool, bulk,
    entries::{self, NewRevision},
    projects,
    scope::{self, ProjectTransaction},
    work,
};
use common::TestDb;
use serde_json::json;
use uuid::Uuid;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

/// Alice's project and its production environment.
async fn project(db: &TestDb) -> (Uuid, Uuid, Uuid) {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let owner = db.owner().await;
    let environment = sqlx::query_scalar!(
        "SELECT id FROM environments WHERE project_id = $1 AND slug = 'production'",
        project
    )
    .fetch_one(&owner)
    .await
    .expect("environment");
    owner.close().await;
    (alice, project, environment)
}

/// One stored asset with a pending verify job of the given key, as the table owner — the
/// shape `assets::insert` writes, made directly so the byte bound can be filled cheaply.
async fn stored_asset(db: &TestDb, alice: Uuid, project: Uuid, environment: Uuid, size: i64) {
    let owner = db.owner().await;
    let asset = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO assets (project_id, environment_id, id, state, file_name,
                             declared_type, size_bytes, quarantine_key, created_by)
         VALUES ($1, $2, $3, 'usable', 'a', 'application/octet-stream', $4, $5, $6)",
    )
    .bind(project)
    .bind(environment)
    .bind(asset)
    .bind(size)
    .bind(format!("quarantine/{asset}"))
    .bind(alice)
    .execute(&owner)
    .await
    .expect("asset");
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind,
                                 idempotency_key, deadline_at)
         VALUES ($1, $2, gen_random_uuid(), $3, 'verify', $4, now() + interval '1 hour')",
    )
    .bind(project)
    .bind(environment)
    .bind(asset)
    .bind(format!("verify:{asset}"))
    .execute(&owner)
    .await
    .expect("asset job");
    owner.close().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[allow(
    clippy::too_many_lines,
    reason = "each counted kind gets its own fixture, assertion and drain in sequence"
)]
async fn pending_payload_counts_stored_bytes_and_drains_as_work_finishes() {
    let db = TestDb::new().await;
    let (alice, project, environment) = project(&db).await;

    // An empty installation holds no pending payload.
    let baseline = work::install_payload(&db.app).await.expect("payload");
    assert_eq!(baseline, 0);

    // A pending verify job counts its asset's recorded size plus its idempotency key —
    // exactly what `colophon_pending_payload` documents it weighs (D-135).
    let owner = db.owner().await;
    let asset = Uuid::now_v7();
    sqlx::query(
        "INSERT INTO assets (project_id, environment_id, id, state, file_name,
                             declared_type, size_bytes, quarantine_key, created_by)
         VALUES ($1, $2, $3, 'usable', 'a', 'application/octet-stream', 1000, $4, $5)",
    )
    .bind(project)
    .bind(environment)
    .bind(asset)
    .bind(format!("quarantine/{asset}"))
    .bind(alice)
    .execute(&owner)
    .await
    .expect("asset");
    let key = format!("verify:{asset}");
    sqlx::query(
        "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind,
                                 idempotency_key, deadline_at)
         VALUES ($1, $2, gen_random_uuid(), $3, 'verify', $4, now() + interval '1 hour')",
    )
    .bind(project)
    .bind(environment)
    .bind(asset)
    .bind(&key)
    .execute(&owner)
    .await
    .expect("asset job");
    let held = work::install_payload(&db.app).await.expect("payload");
    assert_eq!(
        held,
        1000 + i64::try_from(key.len()).expect("key length"),
        "recorded size plus the job's key"
    );

    // Finishing the job frees the bytes: the bound refuses admissions, not drains.
    sqlx::query("UPDATE asset_jobs SET status = 'succeeded', finished_at = now()")
        .execute(&owner)
        .await
        .expect("finish");
    let drained = work::install_payload(&db.app).await.expect("payload");
    assert_eq!(drained, 0, "terminal work holds no payload");

    // An open bulk job counts its operation document; its pending items' inputs count too —
    // written through the real create and append so the post-write check runs where the API
    // runs it.
    let mut tx = enter(&db.app, alice, project).await;
    let operation = json!({"collection": "articles", "set": {"title": "Hi"}});
    let job = bulk::create(
        &mut tx,
        &bulk::NewJob {
            id: Uuid::now_v7(),
            kind: "entries",
            operation: operation.clone(),
            manifest_hash: [3; 32],
            principal: "member",
            expected_items: 0,
            seal: false,
        },
    )
    .await
    .expect("bulk job");
    tx.commit().await.expect("commit");
    // `octet_length(operation::text)` is what the function weighs: the bytes jsonb actually
    // stored, not a client's serialization, so measure them the same way.
    let stored = sqlx::query_scalar::<_, i64>(
        "SELECT octet_length(operation::text)::bigint FROM bulk_jobs WHERE id = $1",
    )
    .bind(job.id)
    .fetch_one(&owner)
    .await
    .expect("stored operation");
    let held = work::install_payload(&db.app).await.expect("payload");
    assert_eq!(
        held, stored,
        "an accepting job's operation document is pending payload"
    );

    // An item's input lands under the job's lock, counted by the same post-write check.
    let mut tx = enter(&db.app, alice, project).await;
    let entry = Uuid::now_v7();
    let revision = Uuid::now_v7();
    entries::create(
        &mut tx,
        entry,
        "articles",
        NewRevision {
            id: revision,
            content: r#"{"title":"Hi"}"#,
            content_hash: &[1; 32],
            title: "Hi",
            slug: None,
            schema_version: 1,
            references: &[],
            assets: &[],
            unique: &[],
            visibility: None,
        },
        None,
    )
    .await
    .expect("entry");
    bulk::lock(&mut tx, job.id).await.expect("lock");
    bulk::append(
        &mut tx,
        job.id,
        &[bulk::NewItem {
            key: "k1",
            entry_id: entry,
            expected_revision_id: revision,
            input: json!({"title": "Bulk item"}),
        }],
    )
    .await
    .expect("append");
    tx.commit().await.expect("commit");
    let input = sqlx::query_scalar::<_, i64>(
        "SELECT octet_length(input::text)::bigint FROM bulk_items WHERE job_id = $1 AND key = 'k1'",
    )
    .bind(job.id)
    .fetch_one(&owner)
    .await
    .expect("stored input");
    let held = work::install_payload(&db.app).await.expect("payload");
    assert_eq!(
        held,
        stored + input,
        "a pending item's input joins its job's operation"
    );

    db.drop().await;
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_full_byte_bound_refuses_new_work_until_open_work_drains() {
    let db = TestDb::new().await;
    let (alice, project, environment) = project(&db).await;

    // 52 pending verify jobs at the maximum recorded size hold more than a gibibyte of
    // recorded payload: over the installation's bound on their own.
    for _ in 0..52 {
        stored_asset(&db, alice, project, environment, 20_971_520).await;
    }
    let held = work::install_payload(&db.app).await.expect("payload");
    assert!(
        held > work::MAX_PENDING_PAYLOAD,
        "the fixture fills the bound"
    );

    // Admission refuses before any row is written, by byte count not row count — 52 open
    // rows are nowhere near MAX_OPEN.
    let mut tx = enter(&db.app, alice, project).await;
    let refused = work::admit(&mut tx, 1).await;
    assert!(
        matches!(refused, Err(DbError::PayloadFull { .. })),
        "over the byte bound admission answers PayloadFull, got {refused:?}"
    );
    tx.transaction.rollback().await.expect("rollback");

    // And a real write path refuses the same way: `bulk::create` admits, writes, checks —
    // its own bytes counted — and the bound answers before commit.
    let mut tx = enter(&db.app, alice, project).await;
    let refused = bulk::create(
        &mut tx,
        &bulk::NewJob {
            id: Uuid::now_v7(),
            kind: "entries",
            operation: json!({"collection": "articles"}),
            manifest_hash: [4; 32],
            principal: "member",
            expected_items: 0,
            seal: false,
        },
    )
    .await;
    assert!(
        matches!(refused, Err(DbError::PayloadFull { .. })),
        "a manifest's bytes are refused too, got {refused:?}"
    );
    tx.transaction.rollback().await.expect("rollback");

    // Draining the open work frees the room: the bound refuses admissions, not recovery.
    let owner = db.owner().await;
    sqlx::query("UPDATE asset_jobs SET status = 'succeeded', finished_at = now()")
        .execute(&owner)
        .await
        .expect("drain");
    owner.close().await;
    let mut tx = enter(&db.app, alice, project).await;
    work::admit(&mut tx, 1).await.expect("room again");
    tx.commit().await.expect("commit");

    db.drop().await;
}
