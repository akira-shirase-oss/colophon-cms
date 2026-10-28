//! The simulator's honest boundary (docs/modules/10-sim.md, section 6): its store is a model of
//! this crate. Here, simulated runs are replayed against real PostgreSQL, save by save, through
//! the same calls the API makes (a member's project transaction, the idempotency claim, the
//! conditional save), and every answer must match the model's: a fresh save, a replay of the
//! same revision, a stale save naming the same current revision. Needs the local stack.
#![expect(
    clippy::expect_used,
    reason = "database tests: a failed connection or setup step is a test failure"
)]

mod common;

use std::collections::BTreeMap;

use colophon_db::{
    entries::{self, NewRevision, Saved},
    idempotency::{self, Claim},
    projects, scope,
};
use colophon_sim::{Config, Mutation, Reply, Report, RevisionId, Save};
use common::TestDb;
use uuid::Uuid;

/// 32 deterministic bytes for a string: the database only compares these, never checks them.
fn digest(text: &str) -> [u8; 32] {
    let mut out = [0_u8; 32];
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for (index, slot) in out.iter_mut().enumerate() {
        for byte in text.bytes().chain(u8::try_from(index).ok()) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        *slot = hash.to_le_bytes()[index % 8];
    }
    out
}

struct World {
    projects: BTreeMap<u64, Uuid>,
    identities: BTreeMap<u64, Uuid>,
    entries: BTreeMap<u64, Uuid>,
    revisions: BTreeMap<RevisionId, Uuid>,
}

/// What the real adapter answered, in the model's terms.
#[derive(Debug, PartialEq, Eq)]
enum Real {
    Saved { revision: Uuid, replayed: bool },
    Stale { current: Uuid },
    KeyReused,
    NotFound,
}

async fn apply(db: &TestDb, world: &World, save: &Save) -> Real {
    let identity = world.identities[&save.identity];
    let project = world.projects[&save.project];
    let mut tx = scope::begin_in_project(&db.app, identity, project, "production")
        .await
        .expect("begin")
        .expect("a member");
    let key = format!("sim-{:016}", save.key);
    let request = digest(&format!(
        "{}\n{}\n{}",
        save.entry, save.expected, save.content
    ));
    match idempotency::claim(&mut tx, "entry.save", &key, &request)
        .await
        .expect("claim")
    {
        Claim::Replay { response, .. } => {
            return Real::Saved {
                revision: Uuid::parse_str(&response).expect("a recorded revision id"),
                replayed: true,
            };
        }
        Claim::Mismatch => return Real::KeyReused,
        Claim::Fresh => {}
    }
    let Some(entry) = world.entries.get(&save.entry) else {
        return Real::NotFound;
    };
    let revision = Uuid::now_v7();
    let hash = digest(&save.content);
    let outcome = entries::save(
        &mut tx,
        *entry,
        world.revisions[&save.expected],
        NewRevision {
            id: revision,
            content: &save.content,
            content_hash: &hash,
            title: &save.content,
            slug: None,
            schema_version: 1,
            references: &[],
        },
    )
    .await
    .expect("save");
    match outcome {
        Saved::Saved(_) => {
            idempotency::complete(&mut tx, "entry.save", &key, 200, &revision.to_string())
                .await
                .expect("complete");
            tx.commit().await.expect("commit");
            Real::Saved {
                revision,
                replayed: false,
            }
        }
        Saved::Stale { current } => Real::Stale { current },
        Saved::NotFound => Real::NotFound,
    }
}

/// The model's projects, members and entries, created for real.
async fn world(db: &TestDb, report: &Report) -> World {
    let owner = db.owner().await;
    let mut world = World {
        projects: BTreeMap::new(),
        identities: BTreeMap::new(),
        entries: BTreeMap::new(),
        revisions: BTreeMap::new(),
    };

    // Projects and their members, as the model has them.
    for (project, _, _, _) in &report.setup {
        if world.projects.contains_key(project) {
            continue;
        }
        let creator = db.administrator(&format!("creator-{project}")).await;
        let id = projects::create(&db.app, creator, &format!("sim-{project}"), "Simulated")
            .await
            .expect("project")
            .id;
        world.projects.insert(*project, id);
        world.identities.insert(*project * 1_000_000, creator);
    }
    let people: std::collections::BTreeSet<(u64, u64)> = report
        .log
        .iter()
        .map(|(save, _)| (save.project, save.identity))
        .collect();
    for (project, identity) in people {
        let person = db.person(&format!("i{identity}")).await;
        sqlx::query(
            "INSERT INTO memberships (project_id, identity_id, role) VALUES ($1, $2, 'editor')",
        )
        .bind(world.projects[&project])
        .bind(person)
        .execute(&owner)
        .await
        .expect("membership");
        world.identities.insert(identity, person);
    }
    owner.close().await;

    // The entries, with their first revisions.
    for (project, entry, revision, content) in &report.setup {
        let creator = world.identities[&(*project * 1_000_000)];
        let mut tx =
            scope::begin_in_project(&db.app, creator, world.projects[project], "production")
                .await
                .expect("begin")
                .expect("a member");
        let (id, first) = (Uuid::now_v7(), Uuid::now_v7());
        entries::create(
            &mut tx,
            id,
            "articles",
            NewRevision {
                id: first,
                content,
                content_hash: &digest(content),
                title: content,
                slug: None,
                schema_version: 1,
                references: &[],
            },
        )
        .await
        .expect("entry");
        tx.commit().await.expect("commit");
        world.entries.insert(*entry, id);
        world.revisions.insert(*revision, first);
    }
    world
}

/// Replays a run's saves against Postgres; the number compared, or the first disagreement.
async fn replay(seed: u64, mutation: Mutation) -> Result<usize, String> {
    let report = colophon_sim::run(seed, &Config::default(), mutation);
    let db = TestDb::new().await;
    let mut world = world(&db, &report).await;

    // Every save, in the model's order: the answers must agree.
    for (index, (save, model)) in report.log.iter().enumerate() {
        let real = apply(&db, &world, save).await;
        let agree = match (model, &real) {
            (
                Reply::Saved {
                    revision,
                    replayed: false,
                },
                Real::Saved {
                    revision: id,
                    replayed: false,
                },
            ) => world.revisions.insert(*revision, *id).is_none(),
            (
                Reply::Saved {
                    revision,
                    replayed: true,
                },
                Real::Saved {
                    revision: id,
                    replayed: true,
                },
            )
            | (Reply::Stale { current: revision }, Real::Stale { current: id }) => {
                world.revisions.get(revision) == Some(id)
            }
            (Reply::KeyReused, Real::KeyReused) | (Reply::NotFound, Real::NotFound) => true,
            _ => false,
        };
        if !agree {
            db.drop().await;
            return Err(format!(
                "seed {seed}, save {index} ({save:?}): the model said {model:?}, Postgres {real:?}"
            ));
        }
    }
    let saves = report.log.len();
    db.drop().await;
    Ok(saves)
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn postgres_answers_every_simulated_save_exactly_as_the_model_does() {
    let mut saves = 0;
    for seed in [1, 2, 3, 42] {
        assert!(
            colophon_sim::run(seed, &Config::default(), Mutation::None)
                .violations
                .is_empty(),
            "seed {seed} must pass in the model first"
        );
        saves += replay(seed, Mutation::None)
            .await
            .unwrap_or_else(|problem| panic!("{problem}"));
    }
    assert!(saves > 500, "only {saves} saves compared");
}

#[tokio::test]
#[ignore = "needs the local stack: scripts/stack.sh up"]
async fn a_model_with_a_broken_rule_disagrees_with_postgres() {
    // The model with the base check skipped accepts a stale save that Postgres refuses: the
    // differential test must notice, or it isn't checking anything.
    let disagreement = replay(1, Mutation::SkipBaseCheck).await;
    assert!(
        disagreement
            .as_ref()
            .is_err_and(|problem| problem.contains("Stale")),
        "{disagreement:?}"
    );
}
