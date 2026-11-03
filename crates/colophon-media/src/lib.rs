//! The upload lifecycle (docs/modules/07-media.md): what an intent may declare, how an object is
//! verified, and where objects live.
//!
//! An upload is quarantined bytes until the worker proves them: [`verify_image`] sniffs the real
//! signature (the declared MIME type proves nothing, M2), decodes under pixel and allocation
//! bounds, and re-encodes so no metadata survives — the stripped bytes are what the public key
//! will serve. Verification is synchronous and CPU-bound; callers run it on a blocking thread
//! with their own deadline.

mod asset;
mod store;
mod verify;

pub use asset::{DECLARED_TYPES, Intent, IntentError, MAX_BYTES, State};
pub use store::{Shared, public_key, quarantine_key};
pub use verify::{Reject, Verified, sniff, verify_image};
