//! The operations one bulk manifest item may carry (P5, D-116): drafts only, each one
//! validated exactly as the matching single write is, and applied to the item's named base
//! revision — never silently to a moved draft.
//!
//! An item's `input` document is one of:
//!
//! - `{"op": "update", "content": {…}}` — a whole replacement document, validated against
//!   the collection's current content type exactly like `entries::save` does;
//! - `{"op": "set", "path": "seo/title", "value": …}` — a [`migrate::Op::Fill`] on the
//!   stored document, then the same validation;
//! - `{"op": "move", "from": "a", "to": "b"}` — a [`migrate::Op::Move`], same treatment;
//! - `{"op": "delete"}` — `colophon_delete_entry` on the entry at its base revision.
//!
//! There is deliberately no `publish` or `unpublish` operation: bulk work is drafts-only
//! (D-116). Publishing stays an exact-change approval a person drives.

use serde_json::Value;

use crate::{migrate, schema::Refusal};

/// The most items one chunk may carry (§17): a submit or an append never takes more.
pub const CHUNK_ITEMS: usize = 100;
/// The most bytes one request body may hold (§17): checked at the HTTP boundary too, but
/// the domain bound is what a client can rely on.
pub const CHUNK_BYTES: usize = 1 << 20;
/// The most items one job's manifest may declare in total, across chunks: cancellation
/// and checkpointing work per job and stay bounded (D-116).
pub const MAX_ITEMS: i64 = 10_000;

/// What one item asks the worker to do.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Replace the draft's content with this document.
    Update {
        /// The replacement document (a JSON object).
        content: Value,
    },
    /// The migration operations to run on the stored document: `set` is one
    /// [`migrate::Op::Fill`], `move` one [`migrate::Op::Move`].
    Apply(Vec<migrate::Op>),
    /// Delete the entry at its base revision.
    Delete,
}

impl Op {
    /// Whether the operation needs `content.delete` on the collection rather than
    /// `content.update`: the permission a submitter and the worker both check (D-116).
    pub fn is_delete(&self) -> bool {
        matches!(self, Op::Delete)
    }
}

fn refused(code: &'static str, pointer: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        pointer: pointer.to_owned(),
        message: message.into(),
    }
}

/// Parses one item's `input` document into an [`Op`], cheaply — the document is checked
/// again at apply time, when the entry's current type and draft are known.
///
/// `set` and `move` delegate to [`migrate::parse`], so a bad path or an unknown key is
/// refused with the migration engine's own wording.
#[allow(
    clippy::too_many_lines,
    reason = "each op's own shape, one match arm apiece"
)]
pub fn parse(input: &Value) -> Result<Op, Refusal> {
    let object = input.as_object().ok_or_else(|| {
        refused(
            "invalid_operation",
            "",
            "an item's operation is an object like {\"op\": \"update\", \"content\": ...}",
        )
    })?;
    let op = object.get("op").and_then(Value::as_str);
    match op {
        Some("update") => {
            if let Some(extra) = object
                .keys()
                .find(|key| !matches!(key.as_str(), "op" | "content"))
            {
                return Err(refused(
                    "invalid_operation",
                    &format!("/{extra}"),
                    format!("`{extra}` is not part of an update operation"),
                ));
            }
            let content = object
                .get("content")
                .filter(|content| content.is_object())
                .ok_or_else(|| {
                    refused(
                        "invalid_operation",
                        "/content",
                        "an update operation needs `content`, a JSON object",
                    )
                })?;
            Ok(Op::Update {
                content: content.clone(),
            })
        }
        Some("set") => {
            // A bulk `set` is one migration `fill`: same path grammar, same refusal codes.
            let op = serde_json::json!([{
                "op": "fill",
                "path": object.get("path").cloned().unwrap_or(Value::Null),
                "value": object.get("value").cloned().unwrap_or(Value::Null),
            }]);
            if let Some(extra) = object
                .keys()
                .find(|key| !matches!(key.as_str(), "op" | "path" | "value"))
            {
                return Err(refused(
                    "invalid_operation",
                    &format!("/{extra}"),
                    format!("`{extra}` is not part of a set operation"),
                ));
            }
            if !object.contains_key("value") {
                return Err(refused(
                    "invalid_operation",
                    "/value",
                    "a set operation needs `value`",
                ));
            }
            let ops = migrate::parse(&op).map_err(|mut refusal| {
                // migrate::parse points at "/0/…"; the item's pointer is the field itself.
                refusal.pointer = refusal
                    .pointer
                    .strip_prefix("/0")
                    .unwrap_or(&refusal.pointer)
                    .to_owned();
                refusal
            })?;
            Ok(Op::Apply(ops))
        }
        Some("move") => {
            let op = serde_json::json!([{
                "op": "move",
                "from": object.get("from").cloned().unwrap_or(Value::Null),
                "to": object.get("to").cloned().unwrap_or(Value::Null),
            }]);
            if let Some(extra) = object
                .keys()
                .find(|key| !matches!(key.as_str(), "op" | "from" | "to"))
            {
                return Err(refused(
                    "invalid_operation",
                    &format!("/{extra}"),
                    format!("`{extra}` is not part of a move operation"),
                ));
            }
            let ops = migrate::parse(&op).map_err(|mut refusal| {
                refusal.pointer = refusal
                    .pointer
                    .strip_prefix("/0")
                    .unwrap_or(&refusal.pointer)
                    .to_owned();
                refusal
            })?;
            Ok(Op::Apply(ops))
        }
        Some("delete") => {
            if let Some(extra) = object.keys().find(|key| key.as_str() != "op") {
                return Err(refused(
                    "invalid_operation",
                    &format!("/{extra}"),
                    format!("`{extra}` is not part of a delete operation"),
                ));
            }
            Ok(Op::Delete)
        }
        // Publishing stays out of bulk on purpose: drafts only (D-116).
        Some("publish" | "unpublish") => Err(refused(
            "unknown_operation",
            "/op",
            "bulk work is drafts-only; publishing is an exact-change approval",
        )),
        Some(other) => Err(refused(
            "unknown_operation",
            "/op",
            format!("`{other}` is not an operation; use update, set, move or delete"),
        )),
        None => Err(refused(
            "invalid_operation",
            "/op",
            "an item's operation needs an `op` of update, set, move or delete",
        )),
    }
}

/// Applies `input` to `stored` (a canonical content document), the way the worker does on
/// the item's transaction: `update` returns its document, `set`/`move` transform `stored`,
/// and `delete` never reaches here. The result still needs the collection's
/// `ContentType::validate` before it may be written — parse-time checking is cheap, never
/// authoritative.
pub fn apply(input: &Op, stored: &Value) -> Result<Value, Refusal> {
    match input {
        Op::Update { content } => Ok(content.clone()),
        Op::Apply(ops) => migrate::apply(stored, ops),
        Op::Delete => Err(refused(
            "invalid_operation",
            "",
            "a delete has no document to apply",
        )),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{Op, apply, parse};

    #[test]
    fn update_takes_a_document() {
        let content = json!({"title": "New", "body": {"x": 1}});
        assert_eq!(
            parse(&json!({"op": "update", "content": content.clone()})),
            Ok(Op::Update { content })
        );
        let missing = parse(&json!({"op": "update"})).unwrap_err();
        assert_eq!(
            (missing.code, missing.pointer.as_str()),
            ("invalid_operation", "/content")
        );
        assert!(parse(&json!({"op": "update", "content": [1]})).is_err());
        assert!(parse(&json!({"op": "update", "content": {}, "extra": 1})).is_err());
    }

    #[test]
    fn set_and_move_are_migration_operations() {
        let set = parse(&json!({"op": "set", "path": "seo/title", "value": "Hi"})).unwrap();
        let moved = parse(&json!({"op": "move", "from": "a", "to": "b"})).unwrap();
        let stored = json!({"title": "Old", "seo": {"title": ""}, "a": 1});
        assert_eq!(
            apply(&set, &stored).unwrap(),
            json!({"title": "Old", "a": 1, "seo": {"title": "Hi"}})
        );
        assert_eq!(
            apply(&moved, &stored).unwrap(),
            json!({"title": "Old", "seo": {"title": ""}, "b": 1})
        );
        // A missing parent is a refusal, like the migration engine reports it.
        assert!(
            apply(
                &parse(&json!({"op": "set", "path": "no/such/field", "value": 1})).unwrap(),
                &stored
            )
            .is_err()
        );
        // Bad paths are refused by migrate::parse at submit time too.
        assert!(parse(&json!({"op": "set", "path": "", "value": 1})).is_err());
        assert!(parse(&json!({"op": "move", "from": "a", "to": "a"})).is_err());
    }

    #[test]
    fn delete_stands_alone() {
        assert_eq!(parse(&json!({"op": "delete"})), Ok(Op::Delete));
        assert!(parse(&json!({"op": "delete", "content": {}})).is_err());
        assert!(parse(&json!({"op": "publish"})).is_err());
        assert!(parse(&json!({"op": "unpublish"})).is_err());
        assert!(parse(&json!({"op": "frobnicate"})).is_err());
        assert!(parse(&json!({"other": 1})).is_err());
        assert!(parse(&json!("delete")).is_err());
    }
}
