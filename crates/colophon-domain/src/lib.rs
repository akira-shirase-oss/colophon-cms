//! The rules of Colophon CMS: content model, editorial workflow, releases and
//! every state machine, written as pure Rust with no I/O.
//!
//! Production adapters and the deterministic simulator both drive this crate
//! unchanged, so it must never depend on an async runtime, an HTTP framework or
//! a database driver. `scripts/check-architecture.sh` enforces that in CI.
//!
//! Design: `docs/modules/01-domain.md`. So far: the rich-text validator ([`rich_text`]),
//! project fields ([`project`]), content types, which validate entries' content ([`schema`]),
//! who may do what ([`policy`]), changesets and their review ([`changeset`]), releases
//! ([`release`]), delivery jobs ([`job`]) and where they may go ([`destination`]).

pub mod cache;
pub mod changeset;
pub mod destination;
pub mod job;
pub mod policy;
pub mod project;
pub mod receipt;
pub mod release;
pub mod rich_text;
pub mod schema;
