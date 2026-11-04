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
    content_types::{self, Updated},
    entries::{self, NewRevision, Saved},
    idempotency::{self, Claim},
    projects, scope,
};
use colophon_sim::{Config, Mutation, Op, Reply, Report, RevisionId, Save};
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
    /// Each project's `articles` collection's version: the model's own counter, kept honest by
    /// the real `content_types::update`s the replay makes.
    versions: BTreeMap<u64, i32>,
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
    // As the API's save handler does it: the collection's write lock, then the version it is
    // stamped with, in the same transaction, so a schema change and a save never interleave
    // inside one version (D-100).
    content_types::lock_for_write(&mut tx, "articles")
        .await
        .expect("lock");
    let version = content_types::current(&mut tx, "articles")
        .await
        .expect("current")
        .expect("articles")
        .version;
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
            schema_version: version,
            references: &[],
            assets: &[],
            unique: &[],
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
        versions: BTreeMap::new(),
    };

    // Projects and their members, as the model has them. A new project's built-in `articles`
    // collection is at version 1, as the model starts it.
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
        world.versions.insert(*project, 1);
    }
    let people: std::collections::BTreeSet<(u64, u64)> = report
        .log
        .iter()
        .filter_map(|op| match op {
            Op::Save(save, _) => Some((save.project, save.identity)),
            Op::Change(..) => None,
        })
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
                assets: &[],
                unique: &[],
            },
            None,
        )
        .await
        .expect("entry");
        tx.commit().await.expect("commit");
        world.entries.insert(*entry, id);
        world.revisions.insert(*revision, first);
    }
    world
}

/// A content-type change that landed in the model, replayed as a real compatible update on
/// `articles` (the display name, which no stored document is checked against): the collection's
/// change lock, then the new version on the one named by the read under it — as the API's
/// update does. `Ok` when Postgres moved the version to the model's.
async fn change(db: &TestDb, world: &mut World, project: u64, version: u64) -> Result<(), String> {
    let creator = world.identities[&(project * 1_000_000)];
    let mut tx = scope::begin_in_project(&db.app, creator, world.projects[&project], "production")
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "the creator isn't a member".to_owned())?;
    content_types::lock_for_change(&mut tx, "articles")
        .await
        .map_err(|error| error.to_string())?;
    let now = content_types::current(&mut tx, "articles")
        .await
        .map_err(|error| error.to_string())?
        .ok_or_else(|| "articles is missing".to_owned())?;
    let mut definition: serde_json::Value =
        serde_json::from_str(&now.definition).map_err(|error| error.to_string())?;
    definition["name"] = serde_json::Value::String(format!("Articles v{version}"));
    let definition = definition.to_string();
    match content_types::update(
        &mut tx,
        "articles",
        now.version,
        &definition,
        &digest(&definition),
    )
    .await
    .map_err(|error| error.to_string())?
    {
        Updated::Updated(landed) if landed.version == i32::try_from(version).unwrap_or(0) => {
            tx.commit().await.map_err(|error| error.to_string())?;
            world.versions.insert(project, landed.version);
            Ok(())
        }
        other => Err(format!("update answered {other:?}")),
    }
}

/// Replays a run's operations against Postgres; the number compared, or the first disagreement.
async fn replay(seed: u64, mutation: Mutation) -> Result<usize, String> {
    let report = colophon_sim::run(seed, &Config::default(), mutation);
    let db = TestDb::new().await;
    let mut world = world(&db, &report).await;
    // What a save stamped is read back as the owner: the application login sees a revision
    // only inside a project transaction.
    let owner = db.owner().await;
    let mut saves = 0_usize;

    // Every operation, in the model's order: the answers must agree.
    for (index, op) in report.log.iter().enumerate() {
        let (save, model) = match op {
            Op::Save(save, model) => (save, model),
            Op::Change(project, version) => {
                if let Err(problem) = change(&db, &mut world, *project, *version).await {
                    owner.close().await;
                    db.drop().await;
                    return Err(format!(
                        "seed {seed}, op {index}: the model moved p{project}'s type to v{version}, Postgres says {problem}"
                    ));
                }
                continue;
            }
        };
        saves += 1;
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
            owner.close().await;
            db.drop().await;
            return Err(format!(
                "seed {seed}, save {index} ({save:?}): the model said {model:?}, Postgres {real:?}"
            ));
        }
        // The version the real save stamped on the revision is the one current under the
        // collection's lock — the version the replay's last real update reached.
        if let Real::Saved {
            revision,
            replayed: false,
        } = real
        {
            let stamped =
                sqlx::query_scalar::<_, i32>("SELECT schema_version FROM revisions WHERE id = $1")
                    .bind(revision)
                    .fetch_one(&owner)
                    .await
                    .expect("the stamped version");
            let tracked = world.versions.get(&save.project).copied().unwrap_or(1);
            if stamped != tracked {
                owner.close().await;
                db.drop().await;
                return Err(format!(
                    "seed {seed}, save {index} ({save:?}): Postgres stamped r{revision} with v{stamped}, but p{}'s type was v{tracked}",
                    save.project
                ));
            }
        }
    }
    owner.close().await;
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
