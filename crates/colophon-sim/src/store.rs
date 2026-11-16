//! An in-memory model of the durable store: what `colophon-db` does for entries, revisions and
//! idempotency records, without Postgres (docs/modules/10-sim.md, "Honest boundary").
//!
//! Every operation is atomic, as a transaction is. The rules are the database's:
//! - a save builds only on the current draft (D2), otherwise it is stale and writes nothing;
//! - revisions are never changed, and each names the revision it was saved on (D1);
//! - a key replays its recorded answer for the same request, refuses a different one, and only
//!   successes are recorded;
//! - a project sees only its own entries (S8);
//! - a save is stamped with the content-type version current when it commits, read under the
//!   collection's lock — a schema change and a save never interleave inside one version (D-100).
//!
//! The differential test in `colophon-db` replays the same operations against Postgres and
//! requires the same answers, so the model can't drift from the real adapter unnoticed.

use std::collections::BTreeMap;

/// A project's id.
pub type ProjectId = u64;
/// A person's id.
pub type IdentityId = u64;
/// An entry's id.
pub type EntryId = u64;
/// A revision's id.
pub type RevisionId = u64;

/// A deliberately broken rule, to prove the checkers notice it (S12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mutation {
    /// The store as it should be.
    #[default]
    None,
    /// Saves skip the expected-revision check: a stale save overwrites silently.
    SkipBaseCheck,
    /// Idempotency keys are ignored: a retried request is applied again.
    ForgetKeys,
    /// Reads ignore the project: an id from another project is served.
    LeakAcrossProjects,
    /// A client bug, not a store one: after a crash the editor saves its kept edit with a new
    /// key instead of resending the request it had sent, so an edit that did commit can be
    /// stored again.
    NewKeyAfterCrash,
    /// A save stamps the content-type version the request was validated against — the one the
    /// client saw when it composed the edit — instead of the version current at commit: a save
    /// validated outside the collection's lock writes a revision against a superseded version
    /// (D-100).
    StampSeenVersion,
}

impl Mutation {
    /// Every mutation, for the mutation suite.
    pub const ALL: [Self; 5] = [
        Self::SkipBaseCheck,
        Self::ForgetKeys,
        Self::LeakAcrossProjects,
        Self::NewKeyAfterCrash,
        Self::StampSeenVersion,
    ];

    /// The name used on the command line.
    pub const fn name(self) -> &'static str {
        match self {
            Self::None => "none",
            Self::SkipBaseCheck => "skip-base-check",
            Self::ForgetKeys => "forget-keys",
            Self::LeakAcrossProjects => "leak-across-projects",
            Self::NewKeyAfterCrash => "new-key-after-crash",
            Self::StampSeenVersion => "stamp-seen-version",
        }
    }

    /// The mutation with this name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::None]
            .into_iter()
            .chain(Self::ALL)
            .find(|mutation| mutation.name() == name)
    }
}

/// A write, exactly as a client sends it. Retrying sends the same value again.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Save {
    /// The project the client works in.
    pub project: ProjectId,
    /// Who is asking.
    pub identity: IdentityId,
    /// The entry.
    pub entry: EntryId,
    /// The revision the edit builds on (`If-Match`).
    pub expected: RevisionId,
    /// The idempotency key.
    pub key: u64,
    /// The content-type version the client saw when it composed the edit. A correct store
    /// stamps the version current at commit, not this one — it only matters to a save that
    /// validates outside the collection's lock.
    pub seen_version: u64,
    /// The new content.
    pub content: String,
}

/// The store's answer to a save.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    /// The draft is now `revision`. `replayed` when the answer came from the key's record.
    Saved {
        /// The draft's revision.
        revision: RevisionId,
        /// True for a recorded answer.
        replayed: bool,
    },
    /// The draft moved on to `current`; nothing was written.
    Stale {
        /// The draft's revision.
        current: RevisionId,
    },
    /// The key was used for a different request.
    KeyReused,
    /// No such entry in this project.
    NotFound,
}

/// A stored revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    /// Its entry.
    pub entry: EntryId,
    /// The revision it was saved on top of; `None` for an entry's first.
    pub base: Option<RevisionId>,
    /// Its content.
    pub content: String,
    /// The content-type version it was written against: the version current when the save
    /// committed, read under the collection's lock (D-100).
    pub schema_version: u64,
    /// Who saved it, and with which key (`None` for an entry's first, made by setup).
    pub saved_by: Option<(IdentityId, u64)>,
}

#[derive(Debug, Clone)]
struct Entry {
    project: ProjectId,
    draft: RevisionId,
}

#[derive(Debug, Clone)]
struct Record {
    request: Save,
    revision: RevisionId,
}

/// The durable store. Its state survives every crash in a run.
#[derive(Debug, Clone, Default)]
pub struct Store {
    entries: BTreeMap<EntryId, Entry>,
    revisions: BTreeMap<RevisionId, Revision>,
    records: BTreeMap<(ProjectId, IdentityId, u64), Record>,
    /// Each project's collection's current version (the model keeps one collection per
    /// project): a content-type change bumps it, a save stamps it on its revision.
    versions: BTreeMap<ProjectId, u64>,
    next_id: u64,
    mutation: Mutation,
}

impl Store {
    /// An empty store, optionally with a rule broken on purpose.
    pub fn new(mutation: Mutation) -> Self {
        Self {
            mutation,
            next_id: 1,
            ..Self::default()
        }
    }

    fn id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        id
    }

    /// The version `project`'s collection is at: 1 before any change, one more after each.
    pub fn version(&self, project: ProjectId) -> u64 {
        self.versions.get(&project).copied().unwrap_or(1)
    }

    /// A content-type change that lands, atomically: the collection's version moves forward.
    /// In the API a change too unsafe for a plain update is refused `needs_migration` (D-100)
    /// and only a migration run's own writes move the version — so a change modelled here is
    /// one that always applies.
    pub fn change_type(&mut self, project: ProjectId) -> u64 {
        let next = self.version(project) + 1;
        self.versions.insert(project, next);
        next
    }

    /// Creates an entry in `project` whose draft is a first revision with `content`; the
    /// entry's id and the revision's.
    pub fn create(&mut self, project: ProjectId, content: &str) -> (EntryId, RevisionId) {
        let entry = self.id();
        let revision = self.id();
        self.revisions.insert(
            revision,
            Revision {
                entry,
                base: None,
                content: content.to_owned(),
                schema_version: self.version(project),
                saved_by: None,
            },
        );
        self.entries.insert(
            entry,
            Entry {
                project,
                draft: revision,
            },
        );
        (entry, revision)
    }

    /// Applies a save atomically, as one transaction.
    pub fn save(&mut self, save: &Save) -> Reply {
        let scope = (save.project, save.identity, save.key);
        if self.mutation != Mutation::ForgetKeys
            && let Some(record) = self.records.get(&scope)
        {
            return if record.request == *save {
                Reply::Saved {
                    revision: record.revision,
                    replayed: true,
                }
            } else {
                Reply::KeyReused
            };
        }
        let Some(entry) = self
            .entries
            .get(&save.entry)
            .filter(|entry| entry.project == save.project)
        else {
            return Reply::NotFound;
        };
        if entry.draft != save.expected && self.mutation != Mutation::SkipBaseCheck {
            return Reply::Stale {
                current: entry.draft,
            };
        }
        // The version the revision is stamped with: the one current under the collection's
        // lock, which this atomic save holds. StampSeenVersion models a save that validated
        // outside the lock, against the version the client had seen.
        let schema_version = if self.mutation == Mutation::StampSeenVersion {
            save.seen_version
        } else {
            self.version(save.project)
        };
        let revision = self.id();
        self.revisions.insert(
            revision,
            Revision {
                entry: save.entry,
                base: Some(save.expected),
                content: save.content.clone(),
                schema_version,
                saved_by: Some((save.identity, save.key)),
            },
        );
        if let Some(entry) = self.entries.get_mut(&save.entry) {
            entry.draft = revision;
        }
        self.records.insert(
            scope,
            Record {
                request: save.clone(),
                revision,
            },
        );
        Reply::Saved {
            revision,
            replayed: false,
        }
    }

    /// The draft of `entry` as `project` sees it: its revision and content, or `None` when the
    /// entry doesn't exist there.
    pub fn read(&self, project: ProjectId, entry: EntryId) -> Option<(RevisionId, &str)> {
        let found = self.entries.get(&entry)?;
        if found.project != project && self.mutation != Mutation::LeakAcrossProjects {
            return None;
        }
        let revision = self.revisions.get(&found.draft)?;
        Some((found.draft, revision.content.as_str()))
    }

    /// The stored revision with this id.
    pub fn revision(&self, id: RevisionId) -> Option<&Revision> {
        self.revisions.get(&id)
    }

    /// The draft's chain of bases, from the draft back to the entry's first revision.
    pub fn lineage(&self, entry: EntryId) -> Vec<RevisionId> {
        let mut chain = Vec::new();
        let mut next = self.entries.get(&entry).map(|entry| entry.draft);
        while let Some(id) = next {
            chain.push(id);
            next = self.revisions.get(&id).and_then(|revision| revision.base);
        }
        chain
    }

    /// Every stored revision, in creation order.
    pub fn revisions(&self) -> impl Iterator<Item = (&RevisionId, &Revision)> {
        self.revisions.iter()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn save(entry: EntryId, expected: RevisionId, key: u64, content: &str) -> Save {
        Save {
            project: 1,
            identity: 10,
            entry,
            expected,
            key,
            seen_version: 1,
            content: content.to_owned(),
        }
    }

    #[test]
    fn saves_build_on_the_draft_and_keys_replay() {
        let mut store = Store::new(Mutation::None);
        let (entry, first) = store.create(1, "a");
        let request = save(entry, first, 100, "b");
        let Reply::Saved {
            revision,
            replayed: false,
        } = store.save(&request)
        else {
            panic!("a fresh save")
        };
        assert_eq!(
            store.save(&request),
            Reply::Saved {
                revision,
                replayed: true
            },
            "the same request replays"
        );
        assert_eq!(store.save(&save(entry, first, 100, "c")), Reply::KeyReused);
        assert_eq!(
            store.save(&save(entry, first, 101, "c")),
            Reply::Stale { current: revision }
        );
        assert_eq!(store.lineage(entry), [revision, first]);
    }

    #[test]
    fn another_project_finds_nothing() {
        let mut store = Store::new(Mutation::None);
        let (entry, first) = store.create(1, "a");
        assert_eq!(store.read(2, entry), None);
        let mut elsewhere = save(entry, first, 100, "b");
        elsewhere.project = 2;
        assert_eq!(store.save(&elsewhere), Reply::NotFound);
    }

    #[test]
    fn each_mutation_breaks_its_rule() {
        let mut store = Store::new(Mutation::SkipBaseCheck);
        let (entry, first) = store.create(1, "a");
        store.save(&save(entry, first, 1, "b"));
        assert!(matches!(
            store.save(&save(entry, first, 2, "c")),
            Reply::Saved { .. }
        ));

        let mut store = Store::new(Mutation::ForgetKeys);
        let (entry, first) = store.create(1, "a");
        store.save(&save(entry, first, 1, "b"));
        assert!(matches!(
            store.save(&save(entry, first, 1, "b")),
            Reply::Stale { .. }
        ));

        let mut store = Store::new(Mutation::LeakAcrossProjects);
        let (entry, _) = store.create(1, "a");
        assert!(store.read(2, entry).is_some());
    }

    #[test]
    fn a_save_stamps_the_version_current_at_commit() {
        let mut store = Store::new(Mutation::None);
        let (entry, first) = store.create(1, "a");
        assert_eq!(store.revision(first).map(|r| r.schema_version), Some(1));
        assert_eq!(store.change_type(1), 2);
        // The client still "saw" version 1 when it composed the edit; the save commits on
        // version 2, and the revision is stamped with it.
        let Reply::Saved { revision, .. } = store.save(&save(entry, first, 1, "b")) else {
            panic!("a fresh save")
        };
        assert_eq!(store.revision(revision).map(|r| r.schema_version), Some(2));

        // With the rule broken, the same race stamps the version the client had seen.
        let mut store = Store::new(Mutation::StampSeenVersion);
        let (entry, first) = store.create(1, "a");
        assert_eq!(store.change_type(1), 2);
        let Reply::Saved { revision, .. } = store.save(&save(entry, first, 1, "b")) else {
            panic!("a fresh save")
        };
        assert_eq!(store.revision(revision).map(|r| r.schema_version), Some(1));
    }
}
