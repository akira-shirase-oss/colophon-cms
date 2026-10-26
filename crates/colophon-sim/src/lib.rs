//! Colophon's deterministic simulator (docs/modules/10-sim.md; D-005, D-039).
//!
//! A run is a seed plus a [`Config`]: editors in several projects save shared articles through a
//! network that loses requests and answers, crash and reopen, and after every step the
//! invariants are checked:
//!
//! - **S1**, no acknowledged save is lost: every save an editor was told succeeded is stored as
//!   sent and is still in its article's history from the draft back, and no edit is stored twice;
//! - **S8**, no read crosses a project;
//! - **K**, no client ever reuses a key for a different request.
//!
//! A second scenario, [`run_publishing`] (step 2.4), drives releases, the outbox and workers under
//! leases on a virtual clock, against a webhook receiver behind a faulty network.
//!
//! Everything comes from the seed, so a failure replays exactly (`colophon-sim --seed N
//! --trace`). [`Mutation`]s break one rule on purpose; the suite requires every one to be caught.
//! The store is a model of `colophon-db`, and the differential test there replays a run's
//! [`Report::log`] against Postgres and requires the same answers.

mod publish;
mod rng;
mod sim;
mod store;

pub use publish::{PublishConfig, PublishMutation, PublishReport, PublishStats, run_publishing};
pub use rng::Rng;
pub use sim::{Config, Report, Stats, Violation, run};
pub use store::{
    EntryId, IdentityId, Mutation, ProjectId, Reply, Revision, RevisionId, Save, Store,
};
