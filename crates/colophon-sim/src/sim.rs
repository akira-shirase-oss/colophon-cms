//! One seeded run: editors who collide on shared articles, a network that loses requests and
//! answers, editors who crash and recover from their device copy, and the invariant checks after
//! every step (docs/modules/10-sim.md).
//!
//! An editor behaves like the Studio's saver (D-038): it retries a lost request with the same
//! key, treats a 409 whose current content is its own edit as saved, and on a real conflict
//! either keeps its version (saving on top of theirs with a new key) or takes theirs. After a
//! crash it reopens from its device copy, which holds the request it had sent, key included,
//! and sends exactly that again: if the first one committed, the store replays it.
//!
//! Everything comes from the seed (S10): the same seed and configuration give the same trace,
//! line for line, and the same trace hash.

use std::collections::{BTreeMap, BTreeSet};

use crate::{
    rng::Rng,
    store::{EntryId, IdentityId, Mutation, ProjectId, Reply, RevisionId, Save, Store},
};

/// The shape of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Projects, each with its own entries and editors.
    pub projects: u64,
    /// Entries per project; editors share them, so saves collide.
    pub entries_per_project: u64,
    /// Editors per project.
    pub editors_per_project: u64,
    /// Steps in the run.
    pub steps: usize,
    /// How often a request is lost before it reaches the store, per thousand.
    pub lose_request: u16,
    /// How often an answer is lost after the store applied the request, per thousand.
    pub lose_answer: u16,
    /// How often an editor crashes when chosen to act, per thousand.
    pub crash: u16,
    /// How often an editor in a conflict takes their version instead of keeping its own.
    pub take_theirs: u16,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            projects: 2,
            entries_per_project: 2,
            editors_per_project: 3,
            steps: 400,
            lose_request: 100,
            lose_answer: 100,
            crash: 30,
            take_theirs: 300,
        }
    }
}

/// An invariant that failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// The invariant's id (docs/modules/10-sim.md).
    pub invariant: &'static str,
    /// The step it was found at.
    pub step: usize,
    /// What was seen.
    pub detail: String,
}

/// What happened in a run, so a test can prove the faults were actually exercised.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    /// Saves an editor received a success for.
    pub acknowledged: u64,
    /// Answers that came from an idempotency record.
    pub replayed: u64,
    /// Stale saves (409).
    pub stale: u64,
    /// Requests lost before the store.
    pub lost_requests: u64,
    /// Answers lost after the store applied the request.
    pub lost_answers: u64,
    /// Editor crashes.
    pub crashes: u64,
    /// Reads of another project's entry ids.
    pub probes: u64,
}

/// A finished run.
#[derive(Debug, Clone)]
pub struct Report {
    /// The seed.
    pub seed: u64,
    /// FNV-1a over the trace: equal for equal runs.
    pub trace_hash: u64,
    /// Every step, one line each.
    pub trace: Vec<String>,
    /// Every invariant failure, in order.
    pub violations: Vec<Violation>,
    /// Counts of what happened.
    pub stats: Stats,
    /// The entries created before the run: project, entry, first revision, content.
    pub setup: Vec<(ProjectId, EntryId, RevisionId, String)>,
    /// Every save the store applied, with its answer, in order: what the differential test
    /// replays against Postgres.
    pub log: Vec<(Save, Reply)>,
}

/// A request waiting in the network, or one whose request or answer went missing.
#[derive(Debug, Clone)]
struct Pending {
    save: Save,
    lost: bool,
}

#[derive(Debug, Clone)]
enum State {
    Idle,
    Waiting(Pending),
    /// Crashed: only the device copy survived: the request that was sent, key included.
    Crashed(Save),
}

#[derive(Debug, Clone)]
struct Editor {
    identity: IdentityId,
    project: ProjectId,
    entry: EntryId,
    base: RevisionId,
    edits: u64,
    keys: u64,
    state: State,
}

struct Run<'a> {
    config: &'a Config,
    mutation: Mutation,
    rng: Rng,
    store: Store,
    editors: Vec<Editor>,
    /// Requests from crashed editors that may still arrive; their answers go nowhere.
    orphans: Vec<Save>,
    /// Acknowledged saves per entry: revision and the content the editor sent.
    acked: BTreeMap<EntryId, Vec<(RevisionId, String)>>,
    /// Content of every stored revision: each edit is unique, so a repeat is a double save.
    stored: BTreeSet<String>,
    entries_by_project: BTreeMap<ProjectId, Vec<EntryId>>,
    step: usize,
    trace: Vec<String>,
    violations: Vec<Violation>,
    stats: Stats,
    log: Vec<(Save, Reply)>,
}

/// Runs one seed.
pub fn run(seed: u64, config: &Config, mutation: Mutation) -> Report {
    let mut store = Store::new(mutation);
    let mut setup = Vec::new();
    let mut entries_by_project: BTreeMap<ProjectId, Vec<EntryId>> = BTreeMap::new();
    let mut stored = BTreeSet::new();
    for project in 1..=config.projects {
        for n in 0..config.entries_per_project {
            let content = format!("p{project}-setup-{n}");
            let (entry, revision) = store.create(project, &content);
            stored.insert(content.clone());
            setup.push((project, entry, revision, content));
            entries_by_project.entry(project).or_default().push(entry);
        }
    }
    let mut editors = Vec::new();
    for project in 1..=config.projects {
        let entries = entries_by_project
            .get(&project)
            .cloned()
            .unwrap_or_default();
        for n in 0..config.editors_per_project {
            let identity = project * 100 + n;
            // Round-robin: several editors per entry, so their saves collide.
            let entry = entries
                .get(usize::try_from(n).unwrap_or(0) % entries.len().max(1))
                .copied()
                .unwrap_or_default();
            let base = store
                .read(project, entry)
                .map_or(0, |(revision, _)| revision);
            editors.push(Editor {
                identity,
                project,
                entry,
                base,
                edits: 0,
                keys: 0,
                state: State::Idle,
            });
        }
    }
    let mut run = Run {
        config,
        mutation,
        rng: Rng::new(seed),
        store,
        editors,
        orphans: Vec::new(),
        acked: BTreeMap::new(),
        stored,
        entries_by_project,
        step: 0,
        trace: Vec::new(),
        violations: Vec::new(),
        stats: Stats::default(),
        log: Vec::new(),
    };
    for step in 0..config.steps {
        run.step = step;
        run.act();
        run.check();
        // The first failure is the one to replay; later ones are its consequences.
        if !run.violations.is_empty() {
            break;
        }
    }
    let trace_hash = fnv1a(&run.trace);
    Report {
        seed,
        trace_hash,
        trace: run.trace,
        violations: run.violations,
        stats: run.stats,
        setup,
        log: run.log,
    }
}

/// FNV-1a, 64 bits, over the trace's lines: a fingerprint that is the same on every machine.
fn fnv1a(lines: &[String]) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in lines.iter().flat_map(|line| line.bytes().chain(*b"\n")) {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    hash
}

impl Run<'_> {
    fn note(&mut self, line: &str) {
        self.trace.push(format!("{:>4} {line}", self.step));
    }

    fn fail(&mut self, invariant: &'static str, detail: String) {
        self.note(&format!("VIOLATION {invariant}: {detail}"));
        self.violations.push(Violation {
            invariant,
            step: self.step,
            detail,
        });
    }

    /// One step: an orphaned request arrives, or one editor acts.
    fn act(&mut self) {
        if !self.orphans.is_empty() && self.rng.chance(200) {
            let index = self.rng.below(self.orphans.len());
            let save = self.orphans.swap_remove(index);
            let reply = self.apply(&save);
            self.note(&format!(
                "orphan from i{} arrives: {reply:?}",
                save.identity
            ));
            return;
        }
        let index = self.rng.below(self.editors.len());
        if self.rng.chance(self.config.crash) {
            self.crash(index);
            return;
        }
        let Some(editor) = self.editors.get(index).cloned() else {
            return;
        };
        match editor.state {
            State::Idle => {
                if self.rng.chance(150) {
                    self.probe(index);
                } else {
                    self.edit(index);
                }
            }
            State::Waiting(pending) if pending.lost => {
                // The client's timeout: send exactly the same request again.
                self.note(&format!(
                    "i{} resends key {}",
                    editor.identity, pending.save.key
                ));
                self.set(
                    index,
                    State::Waiting(Pending {
                        lost: false,
                        ..pending
                    }),
                );
            }
            State::Waiting(pending) => self.deliver(index, pending.save),
            State::Crashed(save) => self.recover(index, save),
        }
    }

    fn set(&mut self, index: usize, state: State) {
        if let Some(editor) = self.editors.get_mut(index) {
            editor.state = state;
        }
    }

    fn key(&mut self, index: usize) -> u64 {
        self.editors.get_mut(index).map_or(0, |editor| {
            editor.keys += 1;
            editor.identity * 1_000_000 + editor.keys
        })
    }

    fn edit(&mut self, index: usize) {
        let key = self.key(index);
        let Some(editor) = self.editors.get_mut(index) else {
            return;
        };
        editor.edits += 1;
        let save = Save {
            project: editor.project,
            identity: editor.identity,
            entry: editor.entry,
            expected: editor.base,
            key,
            content: format!("p{}-i{}-n{}", editor.project, editor.identity, editor.edits),
        };
        let line = format!(
            "i{} edits e{} on r{}: {:?} key {}",
            editor.identity, save.entry, save.expected, save.content, save.key
        );
        editor.state = State::Waiting(Pending { save, lost: false });
        self.note(&line);
    }

    /// Applies a save in the store, and keeps the invariant books.
    fn apply(&mut self, save: &Save) -> Reply {
        let reply = self.store.save(save);
        if let Reply::Saved {
            revision,
            replayed: false,
        } = reply
            && !self.stored.insert(save.content.clone())
        {
            self.fail(
                "S1",
                format!("{:?} was stored twice (now as r{revision})", save.content),
            );
        }
        self.log.push((save.clone(), reply.clone()));
        reply
    }

    fn deliver(&mut self, index: usize, save: Save) {
        let identity = save.identity;
        if self.rng.chance(self.config.lose_request) {
            self.stats.lost_requests += 1;
            self.note(&format!("i{identity}'s request is lost before the store"));
            self.set(index, State::Waiting(Pending { save, lost: true }));
            return;
        }
        let reply = self.apply(&save);
        if self.rng.chance(self.config.lose_answer) {
            self.stats.lost_answers += 1;
            self.note(&format!(
                "store answers i{identity} {reply:?}; the answer is lost"
            ));
            self.set(index, State::Waiting(Pending { save, lost: true }));
            return;
        }
        self.note(&format!("store answers i{identity} {reply:?}"));
        self.answer(index, &save, &reply);
    }

    /// The editor reads its answer, as the Studio's saver does.
    fn answer(&mut self, index: usize, save: &Save, reply: &Reply) {
        match *reply {
            Reply::Saved { revision, replayed } => {
                self.stats.acknowledged += 1;
                if replayed {
                    self.stats.replayed += 1;
                }
                self.acked
                    .entry(save.entry)
                    .or_default()
                    .push((revision, save.content.clone()));
                if let Some(editor) = self.editors.get_mut(index) {
                    editor.base = revision;
                    editor.state = State::Idle;
                }
            }
            Reply::Stale { .. } => {
                self.stats.stale += 1;
                self.resolve(index, save.content.clone());
            }
            Reply::KeyReused => self.fail(
                "K",
                format!(
                    "i{} got KeyReused for its own retry of key {}",
                    save.identity, save.key
                ),
            ),
            Reply::NotFound => self.fail(
                "S8",
                format!("i{} lost its own entry e{}", save.identity, save.entry),
            ),
        }
    }

    /// After a 409 or a crash: compare with the current draft and decide.
    fn resolve(&mut self, index: usize, content: String) {
        let Some(editor) = self.editors.get(index).cloned() else {
            return;
        };
        let Some((current, now)) = self
            .store
            .read(editor.project, editor.entry)
            .map(|(revision, content)| (revision, content.to_owned()))
        else {
            self.fail(
                "S8",
                format!("i{} can't read its own entry", editor.identity),
            );
            return;
        };
        if now == content {
            // An earlier attempt of this very edit reached the store: it is saved.
            self.note(&format!(
                "i{} finds its edit already saved as r{current}",
                editor.identity
            ));
            self.rebase(index, current, State::Idle);
        } else if self.rng.chance(self.config.take_theirs) {
            self.note(&format!(
                "i{} takes their version r{current}",
                editor.identity
            ));
            self.rebase(index, current, State::Idle);
        } else {
            let key = self.key(index);
            let save = Save {
                project: editor.project,
                identity: editor.identity,
                entry: editor.entry,
                expected: current,
                key,
                content,
            };
            self.note(&format!(
                "i{} keeps its version, on top of r{current}, key {key}",
                editor.identity
            ));
            self.rebase(
                index,
                current,
                State::Waiting(Pending { save, lost: false }),
            );
        }
    }

    fn rebase(&mut self, index: usize, base: RevisionId, state: State) {
        if let Some(editor) = self.editors.get_mut(index) {
            editor.base = base;
            editor.state = state;
        }
    }

    fn crash(&mut self, index: usize) {
        let Some(editor) = self.editors.get(index).cloned() else {
            return;
        };
        let State::Waiting(pending) = editor.state else {
            return;
        };
        self.stats.crashes += 1;
        // A request already on the wire may still arrive; nobody will hear its answer.
        if !pending.lost {
            self.orphans.push(pending.save.clone());
        }
        self.note(&format!(
            "i{} crashes; its device keeps {:?} on r{}",
            editor.identity, pending.save.content, pending.save.expected
        ));
        self.set(index, State::Crashed(pending.save));
    }

    /// Reopening the editor from the device copy (D-038): send the kept request again, same
    /// key. If it committed before the crash, the store replays it; if not, it applies now or is
    /// stale like any other save.
    fn recover(&mut self, index: usize, save: Save) {
        let Some(editor) = self.editors.get(index).cloned() else {
            return;
        };
        if self.mutation == Mutation::NewKeyAfterCrash {
            let current = self
                .store
                .read(editor.project, editor.entry)
                .map(|(revision, _)| revision);
            if current == Some(save.expected) {
                let key = self.key(index);
                self.note(&format!(
                    "i{} reopens and saves its kept edit with a new key {key}",
                    editor.identity
                ));
                self.set(
                    index,
                    State::Waiting(Pending {
                        save: Save { key, ..save },
                        lost: false,
                    }),
                );
            } else {
                self.note(&format!(
                    "i{} reopens on an older revision",
                    editor.identity
                ));
                self.resolve(index, save.content);
            }
            return;
        }
        self.note(&format!(
            "i{} reopens and resends key {}",
            editor.identity, save.key
        ));
        self.set(index, State::Waiting(Pending { save, lost: false }));
    }

    /// An editor asks for every entry id of another project.
    fn probe(&mut self, index: usize) {
        let Some(editor) = self.editors.get(index).cloned() else {
            return;
        };
        self.stats.probes += 1;
        let foreign: Vec<EntryId> = self
            .entries_by_project
            .iter()
            .filter(|(project, _)| **project != editor.project)
            .flat_map(|(_, entries)| entries.iter().copied())
            .collect();
        for entry in foreign {
            if self.store.read(editor.project, entry).is_some() {
                self.fail(
                    "S8",
                    format!(
                        "i{} in p{} read e{entry} of another project",
                        editor.identity, editor.project
                    ),
                );
            }
        }
        self.note(&format!("i{} probes other projects", editor.identity));
    }

    /// S1 after every step: every acknowledged save is stored as sent and still in its entry's
    /// history, from the draft back (no silent overwrite).
    fn check(&mut self) {
        let mut failures = Vec::new();
        for (entry, acks) in &self.acked {
            let lineage: BTreeSet<RevisionId> = self.store.lineage(*entry).into_iter().collect();
            for (revision, content) in acks {
                match self.store.revision(*revision) {
                    Some(stored) if stored.content == *content && stored.entry == *entry => {
                        if !lineage.contains(revision) {
                            failures.push(format!(
                                "acknowledged r{revision} ({content:?}) is no longer in e{entry}'s history: overwritten"
                            ));
                        }
                    }
                    _ => failures.push(format!(
                        "acknowledged r{revision} ({content:?}) isn't stored as sent"
                    )),
                }
            }
        }
        for detail in failures {
            self.fail("S1", detail);
        }
    }
}
