//! The plan-level operations a running content migration applies to each item's document
//! before the target definition validates it (docs/modules/01-domain.md, "Migrations";
//! D-101). Pure JSON transforms, like the rest of this crate: no I/O, no async, no database.
//!
//! A plan's `operations` are a list applied in order to the item's expected revision's
//! canonical content:
//!
//! - `{"op": "fill", "path": "<field path>", "value": <json>}` sets the field at `path` to
//!   `value`, creating the field itself but never its parents;
//! - `{"op": "move", "from": "<field path>", "to": "<field path>"}` renames the field: an
//!   absent source is a no-op (entries that never had it are fine), an occupied target is a
//!   refusal, so a move can never silently overwrite a field someone filled.
//!
//! Paths use the grammar `revision_references.field` uses (migration 0025): a field id, then
//! field ids and one-or-two-digit list positions joined by `/` — `seo/title`, `authors/0` —
//! at most nine segments. Malformed or unknown operations are a [`Refusal`], which the
//! caller turns into a `needs_input` item problem a person resolves.

use serde_json::Value;

use crate::schema::{Refusal, is_field_id};

/// The most segments a path may hold: `revision_references.field`'s bound (migration 0025).
const MAX_DEPTH: usize = 9;

/// One step of a path inside a content document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Segment {
    /// A field id, as schema.rs writes it.
    Field(String),
    /// A position in a list.
    Index(usize),
}

impl Segment {
    /// The segment as a path spells it, for pointers in refusals.
    fn text(&self) -> String {
        match self {
            Segment::Field(field) => field.clone(),
            Segment::Index(index) => index.to_string(),
        }
    }
}

/// One declared operation of a migration plan.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// Set the field at `path` to `value`.
    Fill {
        /// Where the value goes.
        path: Vec<Segment>,
        /// What it becomes.
        value: Value,
    },
    /// Move the value at `from` to `to`, removing it at `from`.
    Move {
        /// Where it comes from.
        from: Vec<Segment>,
        /// Where it goes.
        to: Vec<Segment>,
    },
}

/// A refusal inside `operations`, with `at` for its pointer.
fn refused(code: &'static str, at: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        pointer: at.to_owned(),
        message: message.into(),
    }
}

/// A path's segments: a field id, then field ids and one-or-two-digit positions joined by
/// `/`, at most [`MAX_DEPTH`] segments. `at` is where the text sits in `operations`, so a
/// refusal can point at it.
fn path(text: Option<&Value>, at: &str) -> Result<Vec<Segment>, Refusal> {
    let bad = |message: &str| refused("invalid_path", at, message.to_owned());
    let Some(text) = text.and_then(Value::as_str) else {
        return Err(bad("a path is a string like `seo/title` or `authors/0`"));
    };
    let parts: Vec<&str> = text.split('/').collect();
    if parts.len() > MAX_DEPTH || parts.iter().any(|part| part.is_empty()) {
        return Err(bad("a path is field ids and positions joined by `/`"));
    }
    let mut segments = Vec::with_capacity(parts.len());
    for (index, part) in parts.iter().enumerate() {
        if is_field_id(part) {
            segments.push(Segment::Field((*part).to_owned()));
        } else if index > 0 && part.len() <= 2 && part.bytes().all(|byte| byte.is_ascii_digit()) {
            // A position only after the first segment, and 1–2 digits like the schema says.
            let Ok(position) = part.parse::<usize>() else {
                return Err(bad("a path position is a number between 0 and 99"));
            };
            segments.push(Segment::Index(position));
        } else {
            return Err(bad(
                "a path's first segment is a field id; the rest are field ids or positions",
            ));
        }
    }
    Ok(segments)
}

/// The path as a JSON pointer, for refusals that point at where applying failed.
fn at(path: &[Segment]) -> String {
    let mut pointer = String::new();
    for segment in path {
        pointer.push('/');
        pointer.push_str(&segment.text());
    }
    pointer
}

/// `{"op": "fill", "path": p, "value": v}` at `index` in `operations`.
fn fill_at(item: &Value, index: usize) -> Result<Op, Refusal> {
    let here = format!("/{index}");
    let object = item.as_object().ok_or_else(|| {
        refused(
            "invalid_operation",
            &here,
            "an operation is an object like {\"op\": \"fill\", \"path\": ..., \"value\": ...}",
        )
    })?;
    if let Some(extra) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "op" | "path" | "value"))
    {
        return Err(refused(
            "invalid_operation",
            &format!("{here}/{extra}"),
            format!("`{extra}` is not part of a fill operation"),
        ));
    }
    let missing = |key: &str| {
        refused(
            "invalid_operation",
            &format!("{here}/{key}"),
            format!("a fill operation needs `{key}`"),
        )
    };
    if !object.contains_key("value") {
        return Err(missing("value"));
    }
    let path = path(object.get("path"), &format!("{here}/path"))?;
    if path.is_empty() {
        return Err(missing("path"));
    }
    Ok(Op::Fill {
        path,
        value: object["value"].clone(),
    })
}

/// `{"op": "move", "from": f, "to": t}` at `index` in `operations`.
fn move_at(item: &Value, index: usize) -> Result<Op, Refusal> {
    let here = format!("/{index}");
    let object = item.as_object().ok_or_else(|| {
        refused(
            "invalid_operation",
            &here,
            "an operation is an object like {\"op\": \"move\", \"from\": ..., \"to\": ...}",
        )
    })?;
    if let Some(extra) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "op" | "from" | "to"))
    {
        return Err(refused(
            "invalid_operation",
            &format!("{here}/{extra}"),
            format!("`{extra}` is not part of a move operation"),
        ));
    }
    let missing = |key: &str| {
        refused(
            "invalid_operation",
            &format!("{here}/{key}"),
            format!("a move operation needs `{key}`"),
        )
    };
    let from = path(object.get("from"), &format!("{here}/from"))?;
    if from.is_empty() {
        return Err(missing("from"));
    }
    let to = path(object.get("to"), &format!("{here}/to"))?;
    if to.is_empty() {
        return Err(missing("to"));
    }
    if from == to {
        return Err(refused(
            "same_path",
            &format!("{here}/to"),
            "a move's `to` can't be its `from`",
        ));
    }
    if to.len() > from.len() && to[..from.len()] == from[..] {
        return Err(refused(
            "same_path",
            &format!("{here}/to"),
            "a move's `to` can't be inside its `from`",
        ));
    }
    Ok(Op::Move { from, to })
}

/// Parses a plan's declared `operations`: a list of `fill` and `move` objects, checked
/// cheaply when a plan is made so its approval is bound to operations that make sense.
/// An empty list is fine; `operations` that isn't a list is refused.
pub fn parse(operations: &Value) -> Result<Vec<Op>, Refusal> {
    let list = operations.as_array().ok_or_else(|| {
        refused(
            "invalid_operations",
            "",
            "`operations` is a list of {\"op\": \"fill\"|\"move\", ...} objects",
        )
    })?;
    list.iter()
        .enumerate()
        .map(|(index, item)| {
            let op = item.get("op").and_then(Value::as_str);
            match op {
                Some("fill") => fill_at(item, index),
                Some("move") => move_at(item, index),
                Some(other) => Err(refused(
                    "unknown_operation",
                    &format!("/{index}/op"),
                    format!("`{other}` is not an operation; use `fill` or `move`"),
                )),
                None => Err(refused(
                    "invalid_operation",
                    &format!("/{index}/op"),
                    "an operation needs an `op` of `fill` or `move`",
                )),
            }
        })
        .collect()
}

/// The field holding `path`'s last segment, navigated to: every earlier segment must exist
/// (a field of an object, a position a list really has).
fn parent<'a>(document: &'a mut Value, path: &[Segment]) -> Result<&'a mut Value, Refusal> {
    let mut node = document;
    for (index, segment) in path[..path.len() - 1].iter().enumerate() {
        node = match (segment, node) {
            (Segment::Field(field), Value::Object(map)) => map.get_mut(field),
            (Segment::Index(at), Value::Array(list)) => list.get_mut(*at),
            _ => None,
        }
        .ok_or_else(|| {
            refused(
                "invalid_path",
                &at(&path[..=index]),
                "the path doesn't lead anywhere here",
            )
        })?;
    }
    Ok(node)
}

/// The value at `path`, removed; `None` where it leads nowhere.
fn take(document: &mut Value, path: &[Segment]) -> Option<Value> {
    let holder = parent(document, path).ok()?;
    match (holder, path.last()?) {
        (Value::Object(map), Segment::Field(field)) => map.remove(field),
        (Value::Array(list), Segment::Index(at)) if *at < list.len() => Some(list.remove(*at)),
        _ => None,
    }
}

/// `path` refused: the pointer is the path itself.
fn nowhere(path: &[Segment]) -> Refusal {
    refused(
        "invalid_path",
        &at(path),
        "the path doesn't lead anywhere here",
    )
}

/// `path` occupied: a `move` never overwrites.
fn occupied(path: &[Segment]) -> Refusal {
    refused(
        "occupied",
        &at(path),
        "the place the move targets is already set",
    )
}

/// The value at `path`, navigated read-only; `None` where it leads nowhere.
fn get_in<'a>(document: &'a Value, path: &[Segment]) -> Option<&'a Value> {
    let mut node = document;
    for segment in path {
        node = match (segment, node) {
            (Segment::Field(field), Value::Object(map)) => map.get(field)?,
            (Segment::Index(index), Value::Array(list)) => list.get(*index)?,
            _ => return None,
        };
    }
    Some(node)
}

/// Where a `move` may put its value.
enum Slot {
    /// An object's field.
    Field,
    /// A list's end.
    End,
}

/// `to`'s last segment must name a free place in the document as written: an absent object
/// field, or a list's end. Anywhere a value sits refuses `occupied`.
fn free_at(document: &Value, to: &[Segment]) -> Result<Slot, Refusal> {
    let Some(step) = to.last() else {
        return Err(nowhere(to));
    };
    let holder = get_in(document, &to[..to.len() - 1]).ok_or_else(|| nowhere(to))?;
    match (holder, step) {
        (Value::Object(map), Segment::Field(field)) => {
            if map.contains_key(field) {
                Err(occupied(to))
            } else {
                Ok(Slot::Field)
            }
        }
        (Value::Array(list), Segment::Index(index)) => match index.cmp(&list.len()) {
            std::cmp::Ordering::Equal => Ok(Slot::End),
            std::cmp::Ordering::Less => Err(occupied(to)),
            std::cmp::Ordering::Greater => Err(nowhere(to)),
        },
        _ => Err(nowhere(to)),
    }
}

/// Puts `value` at `path`'s last segment, for `fill`: into an object's field, replacing what
/// was there; onto a list's position, where its end appends.
fn put(document: &mut Value, path: &[Segment], value: Value) -> Result<(), Refusal> {
    let Some(step) = path.last() else {
        return Err(nowhere(path));
    };
    let holder = parent(document, path)?;
    match (holder, step) {
        (Value::Object(map), Segment::Field(field)) => {
            map.insert(field.clone(), value);
            Ok(())
        }
        (Value::Array(list), Segment::Index(index)) if *index < list.len() => {
            list[*index] = value;
            Ok(())
        }
        (Value::Array(list), Segment::Index(index)) if *index == list.len() => {
            list.push(value);
            Ok(())
        }
        _ => Err(nowhere(path)),
    }
}

/// Puts the moved `value` at `to`, in the place [`free_at`] proved free: `to`'s parent can't
/// have gone while its source was taken, because a `move` inside its own source is refused at
/// parse.
fn place(document: &mut Value, to: &[Segment], slot: Slot, value: Value) -> Result<(), Refusal> {
    let holder = parent(document, to)?;
    match (holder, slot) {
        (Value::Object(map), Slot::Field) => {
            if let Some(Segment::Field(field)) = to.last() {
                map.insert(field.clone(), value);
            }
            Ok(())
        }
        (Value::Array(list), Slot::End) => {
            list.push(value);
            Ok(())
        }
        _ => Err(nowhere(to)),
    }
}

/// `document` with `ops` applied in order. The first refusal stops it — the document isn't
/// handed back, so a partially applied plan is never stored.
pub fn apply(document: &Value, ops: &[Op]) -> Result<Value, Refusal> {
    let mut out = document.clone();
    for op in ops {
        match op {
            Op::Fill { path, value } => put(&mut out, path, value.clone())?,
            Op::Move { from, to } => {
                // An entry that never had the field has nothing to move. `to` is checked
                // against the document as written, so the index or field an author named is
                // the one the rule decides on.
                if get_in(&out, from).is_none() {
                    continue;
                }
                let slot = free_at(&out, to)?;
                if let Some(value) = take(&mut out, from) {
                    place(&mut out, to, slot, value)?;
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn ops(value: &Value) -> Vec<Op> {
        parse(value).expect("operations parse")
    }

    #[test]
    fn parses_operations() {
        let parsed = ops(&json!([
            {"op": "fill", "path": "subtitle", "value": "x"},
            {"op": "move", "from": "headline", "to": "title"},
            {"op": "move", "from": "blocks/0/by", "to": "byline"}
        ]));
        assert_eq!(parsed.len(), 3);
        assert!(matches!(&parsed[0], Op::Fill { path, .. } if path.len() == 1));
        assert!(matches!(&parsed[2], Op::Move { from, .. } if from.len() == 3));
        // A missing `op`, an unknown one, and a path that isn't one all refuse.
        for bad in [
            json!([{"path": "x"}]),
            json!([{"op": "rewire", "path": "x", "value": 1}]),
            json!([{"op": "fill", "path": "Not a path", "value": 1}]),
            json!([{"op": "fill", "value": 1}]),
            json!([{"op": "move", "from": "a", "to": "a"}]),
            json!([{"op": "move", "from": "a", "to": "a/b"}]),
            json!([{"op": "move", "from": "a", "to": "a/b/c"}]),
            json!({"op": "fill"}),
        ] {
            assert!(parse(&bad).is_err(), "{bad}");
        }
        assert_eq!(
            parse(&json!([{"op": "wat", "x": 1}])).unwrap_err().code,
            "unknown_operation"
        );
        assert_eq!(parse(&json!([])).expect("empty"), vec![]);
    }

    #[test]
    fn fill_sets_fields() {
        let doc = json!({"title": "T", "seo": {"title": "old"}});
        let filled = apply(
            &doc,
            &ops(&json!([
                {"op": "fill", "path": "subtitle", "value": "new"},
                {"op": "fill", "path": "seo/title", "value": "newer"}
            ])),
        )
        .expect("fill applies");
        assert_eq!(filled["subtitle"], "new");
        assert_eq!(filled["seo"]["title"], "newer");
        // A path that leads nowhere refuses, and nothing is handed back.
        let refused = apply(
            &doc,
            &ops(&json!([{"op": "fill", "path": "seo/deeper/x", "value": 1}])),
        );
        assert_eq!(refused.unwrap_err().code, "invalid_path");
        // A document that isn't an object can't take a field.
        let refused = apply(
            &json!(["x"]),
            &ops(&json!([{"op": "fill", "path": "a", "value": 1}])),
        );
        assert_eq!(refused.unwrap_err().code, "invalid_path");
    }

    #[test]
    fn move_renames_without_overwriting() {
        let doc = json!({"headline": "H", "title": "T", "list": ["a", "b"]});
        let moved = apply(
            &doc,
            &ops(&json!([{"op": "move", "from": "headline", "to": "lede"}])),
        )
        .expect("move applies");
        assert_eq!(moved["lede"], "H");
        assert!(moved.get("headline").is_none());
        // An absent source is a no-op; an occupied target refuses.
        let skipped = apply(
            &doc,
            &ops(&json!([{"op": "move", "from": "missing", "to": "lede"}])),
        )
        .expect("absent source is a no-op");
        assert_eq!(skipped, doc);
        let refused = apply(
            &doc,
            &ops(&json!([{"op": "move", "from": "headline", "to": "title"}])),
        );
        assert_eq!(refused.unwrap_err().code, "occupied");
        // Renaming into a parent that exists is occupied too, not a nested-path refusal.
        let refused = apply(
            &doc,
            &ops(&json!([{"op": "move", "from": "list/0", "to": "list"}])),
        );
        assert_eq!(refused.unwrap_err().code, "occupied");
        // Inside a list a move may only land at the end; anywhere set is occupied.
        let moved = apply(
            &doc,
            &ops(&json!([{"op": "move", "from": "list/0", "to": "extra/0"}])),
        );
        assert_eq!(moved.unwrap_err().code, "invalid_path");
        let appended = apply(
            &doc,
            &ops(&json!([
                {"op": "move", "from": "headline", "to": "list/2"}
            ])),
        )
        .expect("append applies");
        assert_eq!(appended["list"], json!(["a", "b", "H"]));
        assert!(appended.get("headline").is_none());
    }

    #[test]
    fn ops_apply_in_order() {
        let doc = json!({"a": "1"});
        let out = apply(
            &doc,
            &ops(&json!([
                {"op": "move", "from": "a", "to": "b"},
                {"op": "fill", "path": "c", "value": 3}
            ])),
        )
        .expect("apply");
        assert_eq!(out, json!({"b": "1", "c": 3}));
    }
}
