//! `colophon/rich-text` version 1: the stored format of every rich-text field
//! (`docs/formats/rich-text-v1.md`, D-021).
//!
//! [`validate`] is the authority. It checks a JSON value against the allowlist and the limits,
//! reports the first problem as a [`Rejection`] (a stable code and a JSON Pointer), and on
//! success returns the document in canonical form, the exact bytes Colophon stores. The shared
//! fixtures in `fixtures/rich-text/v1` are the contract with the editor and the renderer.

mod href;
pub(crate) mod json;
pub(crate) mod pointer;
mod validate;

use std::fmt;

pub use href::is_allowed_href;
pub use validate::validate;

/// The value of the envelope's `schema` field.
pub const SCHEMA: &str = "colophon/rich-text";
/// The only version this validator accepts.
pub const VERSION: u64 = 1;
/// The deepest nesting allowed; the `doc` node is depth 1.
pub const MAX_DEPTH: usize = 16;
/// The largest canonical document, in bytes (256 KiB).
pub const MAX_BYTES: usize = 262_144;
/// The most nodes a document may have, counting every node including text.
pub const MAX_NODES: usize = 10_000;
/// The longest alt text, in Unicode code points.
pub const MAX_ALT_CHARS: usize = 300;
/// The longest link, in Unicode code points.
pub const MAX_HREF_CHARS: usize = 2048;

/// Why a document was refused. Stable: clients and the fixtures branch on these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum RejectCode {
    /// `schema` isn't `colophon/rich-text`.
    UnsupportedSchema,
    /// `version` isn't 1.
    UnsupportedVersion,
    /// A value has the wrong JSON type.
    WrongType,
    /// A required field is absent.
    MissingField,
    /// A field the format doesn't define.
    UnknownField,
    /// A node type outside the allowlist.
    UnknownNode,
    /// A node in a position it isn't allowed, or required content missing.
    InvalidContent,
    /// A mark type outside the allowlist.
    UnknownMark,
    /// Marks on code-block text, `code` with another mark, or a repeated mark.
    InvalidMarks,
    /// An attribute value outside its rule.
    InvalidAttribute,
    /// A link that isn't an allowed scheme, path or fragment.
    UnsafeLink,
    /// A text node with an empty string.
    EmptyText,
    /// Nesting beyond [`MAX_DEPTH`].
    TooDeep,
    /// Over [`MAX_BYTES`] of canonical JSON.
    TooLarge,
    /// Over [`MAX_NODES`] nodes.
    TooManyNodes,
}

impl RejectCode {
    /// The wire representation, as the format document and the fixtures spell it.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UnsupportedSchema => "unsupported_schema",
            Self::UnsupportedVersion => "unsupported_version",
            Self::WrongType => "wrong_type",
            Self::MissingField => "missing_field",
            Self::UnknownField => "unknown_field",
            Self::UnknownNode => "unknown_node",
            Self::InvalidContent => "invalid_content",
            Self::UnknownMark => "unknown_mark",
            Self::InvalidMarks => "invalid_marks",
            Self::InvalidAttribute => "invalid_attribute",
            Self::UnsafeLink => "unsafe_link",
            Self::EmptyText => "empty_text",
            Self::TooDeep => "too_deep",
            Self::TooLarge => "too_large",
            Self::TooManyNodes => "too_many_nodes",
        }
    }
}

impl fmt::Display for RejectCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A refused document: what is wrong, and where (a JSON Pointer; empty means the whole value).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("rich text rejected: {code} at {}", if .pointer.is_empty() { "the document root" } else { .pointer.as_str() })]
pub struct Rejection {
    /// The stable code.
    pub code: RejectCode,
    /// RFC 6901 JSON Pointer to the offending value.
    pub pointer: String,
}

/// A valid document, held in canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RichText {
    canonical: String,
    nodes: usize,
    depth: usize,
}

impl RichText {
    /// The canonical JSON: the exact bytes to store.
    pub fn canonical_json(&self) -> &str {
        &self.canonical
    }

    /// The number of nodes, including text nodes.
    pub fn nodes(&self) -> usize {
        self.nodes
    }

    /// The deepest nesting; the `doc` node is depth 1.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// The size of the canonical JSON, in bytes.
    pub fn bytes(&self) -> usize {
        self.canonical.len()
    }
}
