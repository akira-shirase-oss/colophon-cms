//! Table retention against real PostgreSQL (spec §27, D-135): one bounded pass removes dead
//! state — terminal work and its receipts, the settled ledger, the audit log, dead sign-in
//! and setup rows — while anything still named survives: a reservation's job, a bulk job's
//! items, an accepted invitation, work still open. A second pass over the same database
//! removes nothing. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::time::Duration;

use colophon_db::{
    PgPool, changesets, destinations,
    entries::{self, NewRevision},
    projects, releases,
    retention::{self, Retention},
    schedules::{self, Created, NewSchedule},
    scope::{self, ProjectTransaction},
};
use common::TestDb;
use sqlx::AssertSqlSafe;
use sqlx::types::time::OffsetDateTime;
use uuid::Uuid;

const HOUR: Duration = Duration::from_secs(3600);
const BATCH: i64 = 500;

async fn enter(pool: &PgPool, identity: Uuid, project: Uuid) -> ProjectTransaction {
    scope::begin_in_project(pool, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member")
}

struct Fixture {
    alice: Uuid,
    project: Uuid,
    environment: Uuid,
    entry: Uuid,
    revision: Uuid,
    changeset: Uuid,
    review: Uuid,
}

/// Alice's project with two delivery destinations and one committed release: two real jobs,
/// plus an entry, an approved changeset and its review for the schedule fixture.
async fn released(db: &TestDb) -> Fixture {
    let alice = db.administrator("alice").await;
    let project = projects::create(&db.app, alice, "alpha", "Alpha")
        .await
        .expect("created")
        .id;
    let mut tx = enter(&db.app, alice, project).await;
    for n in 0..2 {
        destinations::create(
            &mut tx,
            "webhook",
            &format!("hook {n}"),
            &format!("https://hooks.example.com/{n}"),
            None,
        )
        .await
        .expect("destination");
    }
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
    let pins = changesets::pins(&mut tx, &[entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "One", &pins, &[2; 32], None)
        .await
        .expect("changeset");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[2; 32], "approve", "", None)
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    releases::commit(&mut tx, 0, changeset, &[2; 32], review, HOUR)
        .await
        .expect("released");
    tx.commit().await.expect("commit");
    let owner = db.owner().await;
    let environment = sqlx::query_scalar!(
        "SELECT id FROM environments WHERE project_id = $1 AND slug = 'production'",
        project
    )
    .fetch_one(&owner)
    .await
    .expect("environment");
    owner.close().await;
    Fixture {
        alice,
        project,
        environment,
        entry,
        revision,
        changeset,
        review,
    }
}

/// Another approved changeset, for a second schedule (a changeset takes one at a time).
async fn approved(db: &TestDb, fix: &Fixture) -> (Uuid, Uuid) {
    let mut tx = enter(&db.app, fix.alice, fix.project).await;
    let pins = changesets::pins(&mut tx, &[fix.entry]).await.expect("pins");
    let changeset = Uuid::now_v7();
    changesets::create(&mut tx, changeset, "Next", &pins, &[7; 32], None)
        .await
        .expect("changeset");
    changesets::submit(&mut tx, changeset)
        .await
        .expect("submitted");
    changesets::lock(&mut tx, changeset).await.expect("lock");
    changesets::review(&mut tx, changeset, &[7; 32], "approve", "", None)
        .await
        .expect("approved");
    let review = changesets::reviews(&mut tx, changeset, None)
        .await
        .expect("reviews")[0]
        .id;
    tx.commit().await.expect("commit");
    (changeset, review)
}

/// A schedule through the real create, an hour out on `approval` of `changeset`.
async fn schedule(db: &TestDb, fix: &Fixture, changeset: Uuid, approval: Uuid) {
    let mut tx = enter(&db.app, fix.alice, fix.project).await;
    let made = schedules::create(
        &mut tx,
        &NewSchedule {
            id: Uuid::now_v7(),
            changeset,
            hash: &[9; 32],
            approval,
            local_time: "2027-11-03T09:00:00",
            time_zone: "Europe/London",
            choice: None,
            run_at: OffsetDateTime::now_utc() + time::Duration::hours(1),
        },
    )
    .await
    .expect("schedule");
    assert!(matches!(made, Created::Done(_)));
    tx.commit().await.expect("commit");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
#[allow(
    clippy::too_many_lines,
    reason = "one fixture row per retained table, then the sweep and its assertions — \
              the table-per-paragraph shape is the point of the test"
)]
async fn a_pass_removes_dead_state_but_never_what_is_still_named() {
    let db = TestDb::new().await;
    let fix = released(&db).await;
    let owner = db.owner().await;

    // The two jobs the release committed: job_a ages out terminal and goes; job_b is just
    // as old but a settled reservation still names it, so it stays.
    let jobs = sqlx::query_scalar::<_, Uuid>("SELECT id FROM jobs ORDER BY id")
        .fetch_all(&owner)
        .await
        .expect("jobs");
    let (job_a, job_b) = (jobs[0], jobs[1]);
    sqlx::query("UPDATE jobs SET status = 'succeeded', finished_at = now() - interval '40 days'")
        .execute(&owner)
        .await
        .expect("aged jobs");
    let destination =
        sqlx::query_scalar::<_, Uuid>("SELECT destination_id FROM jobs WHERE id = $1")
            .bind(job_a)
            .fetch_one(&owner)
            .await
            .expect("destination");
    let generation = sqlx::query_scalar::<_, i64>("SELECT generation FROM jobs WHERE id = $1")
        .bind(job_a)
        .fetch_one(&owner)
        .await
        .expect("generation");
    // An aged receipt on job_a goes; a fresh one on job_b stays with its job.
    for (job, observed) in [(job_a, "40 days"), (job_b, "0 seconds")] {
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO observations (project_id, environment_id, id, job_id, destination_id,
                                       generation, location, observed_at)
             VALUES ($1, $2, gen_random_uuid(), $3, $4, $5, 'https://w.test',
                     now() - interval '{observed}')",
        )))
        .bind(fix.project)
        .bind(fix.environment)
        .bind(job)
        .bind(destination)
        .bind(generation)
        .execute(&owner)
        .await
        .expect("observation");
    }
    // A settled reservation 10 days old keeps job_b; one 100 days old is itself swept —
    // and stops pinning nothing: job_a carries no reservation at all.
    for (job, settled) in [(job_b, "10 days"), (job_a, "100 days")] {
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO budget_reservations (project_id, environment_id, id, principal_id,
                                              kind, reserved, settled, state, job_id,
                                              settled_at)
             VALUES ($1, $2, gen_random_uuid(), $3, 'jobs', 1, 1, 'settled', $4,
                     now() - interval '{settled}')",
        )))
        .bind(fix.project)
        .bind(fix.environment)
        .bind(fix.alice)
        .bind(job)
        .execute(&owner)
        .await
        .expect("reservation");
    }

    // Three finished bulk jobs: bulk_a goes with its item; bulk_b is pinned like job_b;
    // bulk_c's pin is the ledger row just about to age out, so it goes too.
    for pin in [None, Some("10 days"), Some("100 days")] {
        let job = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO bulk_jobs (project_id, environment_id, id, kind, operation,
                                    manifest_hash, state, created_by, principal,
                                    expected_items, started_at, finished_at)
             VALUES ($1, $2, $3, 'entries', '{\"collection\":\"articles\"}',
                     '\\x00000000000000000000000000000000000000000000000000000000000000ab',
                     'done', $4, 'member', 1, now() - interval '40 days',
                     now() - interval '40 days')",
        )
        .bind(fix.project)
        .bind(fix.environment)
        .bind(job)
        .bind(fix.alice)
        .execute(&owner)
        .await
        .expect("bulk job");
        sqlx::query(
            "INSERT INTO bulk_items (project_id, environment_id, job_id, key, entry_id,
                                     expected_revision_id, input)
             VALUES ($1, $2, $3, 'k', $4, $5, '{\"title\":\"Hi\"}')",
        )
        .bind(fix.project)
        .bind(fix.environment)
        .bind(job)
        .bind(fix.entry)
        .bind(fix.revision)
        .execute(&owner)
        .await
        .expect("bulk item");
        if let Some(age) = pin {
            sqlx::query(AssertSqlSafe(format!(
                "INSERT INTO budget_reservations (project_id, environment_id, id,
                                                 principal_id, kind, reserved, settled,
                                                 state, bulk_job_id, settled_at)
                 VALUES ($1, $2, gen_random_uuid(), $3, 'jobs', 1, 1, 'settled', $4,
                         now() - interval '{age}')",
            )))
            .bind(fix.project)
            .bind(fix.environment)
            .bind(fix.alice)
            .bind(job)
            .execute(&owner)
            .await
            .expect("bulk pin");
        }
    }

    // A terminal verify job goes; the one still inside its keep stays.
    for (name, finished) in [("old", "40 days"), ("new", "1 day")] {
        let asset = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO assets (project_id, environment_id, id, state, file_name,
                                 declared_type, size_bytes, quarantine_key, created_by)
             VALUES ($1, $2, $3, 'usable', $4, 'application/octet-stream', 1000, $5, $6)",
        )
        .bind(fix.project)
        .bind(fix.environment)
        .bind(asset)
        .bind(name)
        .bind(format!("quarantine/{name}"))
        .bind(fix.alice)
        .execute(&owner)
        .await
        .expect("asset");
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO asset_jobs (project_id, environment_id, id, asset_id, kind,
                                     idempotency_key, deadline_at, status, finished_at)
             VALUES ($1, $2, gen_random_uuid(), $3, 'verify', $4, now(), 'succeeded',
                     now() - interval '{finished}')",
        )))
        .bind(fix.project)
        .bind(fix.environment)
        .bind(asset)
        .bind(format!("verify:{name}"))
        .execute(&owner)
        .await
        .expect("asset job");
    }

    // Two finished plans on a fabricated target version: the old one drains with its item.
    sqlx::query(
        "INSERT INTO content_type_versions (project_id, environment_id, collection, version,
                                            previous_version, definition, definition_hash,
                                            created_by)
         VALUES ($1, $2, 'articles', 2, 1, '{}',
                 '\\x0000000000000000000000000000000000000000000000000000000000000002', $3)",
    )
    .bind(fix.project)
    .bind(fix.environment)
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("type version");
    for finished in ["40 days", "1 day"] {
        let plan = Uuid::now_v7();
        sqlx::query(AssertSqlSafe(format!(
            "INSERT INTO migration_plans (project_id, environment_id, id, collection,
                                         base_version, target_version, target_definition,
                                         target_definition_hash, operations, state, plan_hash,
                                         created_by, expected_items, started_at, finished_at)
             VALUES ($1, $2, $3, 'articles', 1, 2, '{{}}',
                     '\\x0000000000000000000000000000000000000000000000000000000000000003',
                     '[]', 'done',
                     '\\x0000000000000000000000000000000000000000000000000000000000000004',
                     $4, 1, now() - interval '{finished}', now() - interval '{finished}')",
        )))
        .bind(fix.project)
        .bind(fix.environment)
        .bind(plan)
        .bind(fix.alice)
        .execute(&owner)
        .await
        .expect("plan");
        sqlx::query(
            "INSERT INTO migration_items (project_id, environment_id, plan_id, entry_id,
                                          expected_revision_id, outcome)
             VALUES ($1, $2, $3, $4, $5, 'ok')",
        )
        .bind(fix.project)
        .bind(fix.environment)
        .bind(plan)
        .bind(fix.entry)
        .bind(fix.revision)
        .execute(&owner)
        .await
        .expect("plan item");
    }

    // Two schedules through the real create: one finishes long ago, one yesterday.
    schedule(&db, &fix, fix.changeset, fix.review).await;
    let (changeset, review) = approved(&db, &fix).await;
    schedule(&db, &fix, changeset, review).await;
    for finished in ["40 days", "1 day"] {
        sqlx::query(AssertSqlSafe(format!(
            "UPDATE schedules SET state = 'published', generation = 1,
                 started_at = now() - interval '{finished}',
                 finished_at = now() - interval '{finished}'
             WHERE id = (SELECT id FROM schedules
                         WHERE finished_at IS NULL
                         ORDER BY created_at LIMIT 1)"
        )))
        .execute(&owner)
        .await
        .expect("age schedule");
    }

    // Auth and flow state: one dead of each kind, one live of each that deserves it.
    sqlx::query(
        "INSERT INTO audit_events (project_id, id, at, actor_id, action, subject)
         VALUES ($1, gen_random_uuid(), now() - interval '100 days', $2, 'entry.save', 'entry'),
                ($1, gen_random_uuid(), now() - interval '1 day', $2, 'entry.save', 'entry')",
    )
    .bind(fix.project)
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("audit");
    sqlx::query(
        "INSERT INTO idempotency_records (project_id, environment_id, identity_id, operation,
                                          key, request_hash, created_at)
         VALUES ($1, $2, $3, 'entry.create', 'aaaaaaaaaaaaaaaa',
                 '\\x0000000000000000000000000000000000000000000000000000000000000005',
                 now() - interval '40 days'),
                ($1, $2, $3, 'entry.create', 'bbbbbbbbbbbbbbbb',
                 '\\x0000000000000000000000000000000000000000000000000000000000000006',
                 now())",
    )
    .bind(fix.project)
    .bind(fix.environment)
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("idempotency");
    sqlx::query(
        "INSERT INTO sessions (id, token_hash, csrf_hash, identity_id, created_at,
                               last_seen_at, expires_at)
         VALUES (gen_random_uuid(),
                 '\\x0000000000000000000000000000000000000000000000000000000000000007',
                 '\\x0000000000000000000000000000000000000000000000000000000000000008',
                 $1, now() - interval '200 days', now() - interval '200 days',
                 now() - interval '100 days'),
                (gen_random_uuid(),
                 '\\x0000000000000000000000000000000000000000000000000000000000000009',
                 '\\x000000000000000000000000000000000000000000000000000000000000000a',
                 $1, now(), now(), now() + interval '30 days')",
    )
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("sessions");
    sqlx::query(
        "INSERT INTO access_tokens (id, token_hash, identity_id, name, created_at,
                                    last_used_at, expires_at)
         VALUES (gen_random_uuid(),
                 '\\x000000000000000000000000000000000000000000000000000000000000000b',
                 $1, 'old', now() - interval '200 days', now() - interval '200 days',
                 now() - interval '100 days'),
                (gen_random_uuid(),
                 '\\x000000000000000000000000000000000000000000000000000000000000000c',
                 $1, 'new', now(), now(), now() + interval '30 days')",
    )
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("tokens");
    // A lapsed invitation nobody took goes; the one that made a member stays.
    sqlx::query(
        "INSERT INTO invitations (id, project_id, token_hash, role, label, invited_by,
                                  created_at, expires_at, accepted_by, accepted_at)
         VALUES (gen_random_uuid(), $1,
                 '\\x000000000000000000000000000000000000000000000000000000000000000d',
                 'editor', 'old invite', $2, now() - interval '200 days',
                 now() - interval '100 days', NULL, NULL),
                (gen_random_uuid(), $1,
                 '\\x000000000000000000000000000000000000000000000000000000000000000e',
                 'editor', 'used invite', $2, now() - interval '200 days',
                 now() - interval '100 days', $2, now() - interval '150 days')",
    )
    .bind(fix.project)
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("invitations");
    sqlx::query(
        "INSERT INTO device_logins (device_hash, user_code, client_name, created_at,
                                   expires_at)
         VALUES ('\\x000000000000000000000000000000000000000000000000000000000000000f',
                 'BCDF-GHJK', 'laptop', now() - interval '2 hours',
                 now() - interval '1 hour')",
    )
    .execute(&owner)
    .await
    .expect("device login");
    sqlx::query(
        "INSERT INTO sign_in_attempts (state_hash, nonce, pkce_verifier, return_to, expires_at)
         VALUES ('\\x0000000000000000000000000000000000000000000000000000000000000010',
                 'nonce-nonce-nonce-1', 'verifier-verifier-verifier-verifier-verifier-v',
                 '/x', now() - interval '1 hour')",
    )
    .execute(&owner)
    .await
    .expect("sign-in attempt");
    sqlx::query(
        "INSERT INTO setup_attempts (id, identity_id, attempted_at)
         VALUES (gen_random_uuid(), $1, now() - interval '1 hour')",
    )
    .bind(fix.alice)
    .execute(&owner)
    .await
    .expect("setup attempt");
    sqlx::query(
        "INSERT INTO setup_codes (id, code_hash, issued_at, expires_at)
         VALUES (gen_random_uuid(),
                 '\\x0000000000000000000000000000000000000000000000000000000000000011',
                 now() - interval '200 days',
                 now() - interval '200 days' + interval '23 hours')",
    )
    .execute(&owner)
    .await
    .expect("setup code");

    let swept = retention::sweep(&db.app, &Retention::DEFAULT, BATCH)
        .await
        .expect("sweep");

    assert_eq!(swept.reservations, 2, "the two 100-day ledger rows age out");
    assert_eq!(
        swept.jobs, 1,
        "job_a goes; job_b's reservation still names it"
    );
    assert_eq!(
        swept.observations, 1,
        "the aged receipt goes; the fresh one stays"
    );
    assert_eq!(swept.asset_jobs, 1);
    assert_eq!(swept.bulk_items, 2, "bulk_a's and bulk_c's items");
    assert_eq!(swept.bulk_jobs, 2, "bulk_a and bulk_c; bulk_b is pinned");
    assert_eq!(swept.migration_items, 1);
    assert_eq!(swept.migration_plans, 1);
    assert_eq!(swept.schedules, 1);
    assert_eq!(swept.audit, 1);
    assert_eq!(swept.idempotency, 1);
    assert_eq!(swept.sessions, 1);
    assert_eq!(swept.access_tokens, 1);
    assert_eq!(swept.invitations, 1);
    assert_eq!(swept.device_logins, 1);
    assert_eq!(swept.sign_in_attempts, 1);
    assert_eq!(swept.setup_attempts, 1);
    assert_eq!(swept.setup_codes, 1);

    // What must still be there: the pinned job and its fresh receipt, the pinned bulk job
    // and its item, every recent row, the accepted invitation.
    let kept = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM jobs")
        .fetch_one(&owner)
        .await
        .expect("jobs left");
    assert_eq!(kept, 1, "job_b alone remains");
    let kept = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM bulk_jobs")
        .fetch_one(&owner)
        .await
        .expect("bulk jobs left");
    assert_eq!(kept, 1, "bulk_b alone remains");
    let kept = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM bulk_items")
        .fetch_one(&owner)
        .await
        .expect("items left");
    assert_eq!(kept, 1, "bulk_b's item survives with it");
    let kept = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM migration_plans")
        .fetch_one(&owner)
        .await
        .expect("plans left");
    assert_eq!(kept, 1, "the recent plan remains");
    let kept = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM schedules")
        .fetch_one(&owner)
        .await
        .expect("schedules left");
    assert_eq!(kept, 1, "the recent schedule remains");
    let accepted = sqlx::query_scalar::<_, i64>(
        "SELECT count(*) FROM invitations WHERE accepted_at IS NOT NULL",
    )
    .fetch_one(&owner)
    .await
    .expect("accepted");
    assert_eq!(accepted, 1);
    let audit = sqlx::query_scalar::<_, i64>("SELECT count(*) FROM audit_events")
        .fetch_one(&owner)
        .await
        .expect("audit left");
    assert_eq!(audit, 1, "the recent audit event remains");

    // A second pass finds nothing: every delete is unconditional, so an empty pass is the
    // steady state, not a stall.
    let again = retention::sweep(&db.app, &Retention::DEFAULT, BATCH)
        .await
        .expect("second sweep");
    assert_eq!(again.total(), 0, "nothing left to remove");

    db.drop().await;
}
