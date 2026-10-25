//! The rules of Colophon CMS: content model, editorial workflow, releases and
//! every state machine, written as pure Rust with no I/O.
//!
//! Production adapters and the deterministic simulator both drive this crate
//! unchanged, so it must never depend on an async runtime, an HTTP framework or
//! a database driver. `scripts/check-architecture.sh` enforces that in CI.
//!
//! Design: `docs/modules/01-domain.md`. So far: the rich-text validator ([`rich_text`]),
//! project fields ([`project`]) and articles, the content of entries ([`entry`]).

pub mod entry;
pub mod project;
pub mod rich_text;
