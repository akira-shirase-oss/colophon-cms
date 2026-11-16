//! Reference population for the query language's `depth` (DX.5, D-107): how a stored
//! reference id becomes a bounded object naming its entry.
//!
//! With `depth` 1 or more, a top-level `reference` field that holds one entry — never a
//! list — has its value rewritten from the stored id string
//! (`"author": "<uuid>"`) to the [`Expanded`] object its id resolved to
//! (`"author": {"id": "<uuid>", "collection": "staff", …}`). With `depth` 2, the
//! resolved entry's own single-reference fields expand one level further, and no more:
//! [`crate::query::MAX_DEPTH`] is the bound, so a cycle of references always ends — the
//! deepest level's own references stay the stored ids they are at `depth` 0.
//!
//! A reference only ever expands to what the read could have reached anyway. An id
//! that doesn't resolve stays the stored string: a target that doesn't exist or, on
//! the published perspective, isn't published at the generation being read — and, on
//! the draft perspective, one the caller's role may not read. Nothing claims the
//! target is readable; nothing claims it is missing. Which of those it was stays
//! unanswerable, as the codebase's other reads keep it (an entry the role can't read
//! isn't found; a referrer in a collection it can't read isn't named).
//!
//! Only the top-level single `reference` fields expand — the kind an `index`ed field
//! may be (D-103): a `many` list and a reference nested in an object, a list or a
//! block keep their stored spellings, the same values a `depth` 0 answer gives.

use std::{
    collections::{BTreeSet, HashMap},
    hash::BuildHasher,
};

use serde_json::{Map, Value};

use crate::schema::{ContentType, FieldKind};

/// One entry a reference resolved to, as its populated object embeds it: `id` is
/// written by the expansion itself from the stored reference, so the object always
/// echoes the exact reference it stands for.
#[derive(Debug, Clone, PartialEq)]
pub struct Expanded {
    /// The entry's collection — the reference's `to`, echoed.
    pub collection: String,
    /// The revision the resolution read: the entry's current draft revision on the
    /// draft perspective, its open publication's revision on the published one.
    pub revision_id: String,
    /// Its title, or empty when the caller may not read the title field — the same
    /// blanking a draft view applies.
    pub title: String,
    /// Its document, embedded under `content`: already narrowed to the caller's read
    /// fields (the draft perspective — the published one's is whole, published
    /// content being public) and already expanded one level shallower where `depth`
    /// remained — which is never more than one, the bound itself.
    pub content: Value,
    /// The fields of the entry's own type the caller may not read — the
    /// `hidden_fields` a draft view reports. Left out of the object when empty, and
    /// always empty on the published perspective, where nothing is hidden.
    pub hidden_fields: Vec<String>,
}

/// Rewrites `document`'s top-level single-reference values with their [`Expanded`]
/// objects: each field `definition` declares a non-list `reference`, whose stored id
/// `resolved` holds. An id it doesn't hold stays the stored string; a value that
/// isn't an id string at all stays as it is. `document` keeps its own key order —
/// only values are replaced.
///
/// One call expands one level: the `content` an [`Expanded`] embeds is whatever its
/// resolver produced — expanded once more itself where `depth` remained, so callers
/// resolve level by level and the deepest level's references stay ids.
pub fn expand<S: BuildHasher>(
    definition: &ContentType,
    document: &mut Map<String, Value>,
    resolved: &HashMap<String, Expanded, S>,
) {
    for field in definition.fields() {
        let FieldKind::Reference { many: false, .. } = &field.kind else {
            continue;
        };
        let Some(id) = document.get(&field.id).and_then(Value::as_str) else {
            continue;
        };
        let Some(expanded) = resolved.get(id) else {
            continue;
        };
        let mut object = Map::new();
        object.insert(
            "collection".to_owned(),
            Value::String(expanded.collection.clone()),
        );
        object.insert("content".to_owned(), expanded.content.clone());
        if !expanded.hidden_fields.is_empty() {
            object.insert(
                "hidden_fields".to_owned(),
                expanded
                    .hidden_fields
                    .iter()
                    .map(|field| Value::String(field.clone()))
                    .collect(),
            );
        }
        object.insert("id".to_owned(), Value::String(id.to_owned()));
        object.insert(
            "revision_id".to_owned(),
            Value::String(expanded.revision_id.clone()),
        );
        object.insert("title".to_owned(), Value::String(expanded.title.clone()));
        document.insert(field.id.clone(), Value::Object(object));
    }
}

/// The entry ids `document`'s top-level single-reference fields name, added to `into`:
/// the values [`expand`] would rewrite, collected so one batched read can resolve
/// them before it runs. Values that aren't id strings — like ones an earlier
/// expansion already rewrote — contribute none.
pub fn collect_ids(
    definition: &ContentType,
    document: &Map<String, Value>,
    into: &mut BTreeSet<String>,
) {
    for field in definition.fields() {
        let FieldKind::Reference { many: false, .. } = &field.kind else {
            continue;
        };
        if let Some(id) = document.get(&field.id).and_then(Value::as_str) {
            into.insert(id.to_owned());
        }
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// A type with one single reference, one `many` reference and one nested one.
    fn definition() -> ContentType {
        ContentType::parse(&json!({
            "name": "Posts", "title_field": "headline", "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true},
                {"id": "author", "label": "Author", "type": "reference", "to": "staff"},
                {"id": "related", "label": "Related", "type": "reference", "to": "articles", "many": true},
                {"id": "meta", "label": "Meta", "type": "object", "fields": [
                    {"id": "see", "label": "See", "type": "reference", "to": "staff"}
                ]}
            ]
        }))
        .expect("a valid definition")
    }

    const ADA: &str = "123e4567-e89b-12d3-a456-426614174000";

    fn ada() -> Expanded {
        Expanded {
            collection: "staff".to_owned(),
            revision_id: "223e4567-e89b-12d3-a456-426614174001".to_owned(),
            title: "Ada".to_owned(),
            content: json!({"name": "Ada", "mentor": "323e4567-e89b-12d3-a456-426614174002"}),
            hidden_fields: Vec::new(),
        }
    }

    /// The document as a mutable object.
    fn document(value: &Value) -> Map<String, Value> {
        value.as_object().expect("an object").clone()
    }

    #[test]
    fn a_single_reference_becomes_the_resolved_object() {
        let mut resolved = HashMap::new();
        resolved.insert(ADA.to_owned(), ada());
        let mut doc = document(
            &json!({"headline": "Hi", "author": ADA, "extra": "423e4567-e89b-12d3-a456-426614174003"}),
        );
        expand(&definition(), &mut doc, &resolved);
        let populated = doc["author"].as_object().expect("an object");
        assert_eq!(populated["id"], json!(ADA));
        assert_eq!(populated["collection"], json!("staff"));
        assert_eq!(populated["revision_id"], json!(ada().revision_id));
        assert_eq!(populated["title"], json!("Ada"));
        assert_eq!(populated["content"], ada().content);
        // No `hidden_fields` when the caller reads every field.
        assert!(!populated.contains_key("hidden_fields"));
        // A field with the same shape but no reference declaration stays a string.
        assert_eq!(doc["extra"], json!("423e4567-e89b-12d3-a456-426614174003"));
    }

    #[test]
    fn an_unresolved_id_and_a_many_or_nested_reference_keep_the_stored_spelling() {
        let mut resolved = HashMap::new();
        resolved.insert(ADA.to_owned(), ada());
        let ben = "323e4567-e89b-12d3-a456-426614174002";
        let mut doc = document(&json!({
            "author": ben,                     // resolves to nothing here
            "related": [ADA],                  // a `many` reference: stays a list of ids
            "meta": {"see": ADA}               // nested: stays as stored
        }));
        expand(&definition(), &mut doc, &resolved);
        assert_eq!(doc["author"], json!(ben));
        assert_eq!(doc["related"], json!([ADA]));
        assert_eq!(doc["meta"], json!({"see": ADA}));
    }

    #[test]
    fn hidden_fields_are_echoed_when_the_caller_cannot_read_them() {
        let mut secret = ada();
        secret.content = json!({"name": "Ada"});
        secret.hidden_fields = vec!["bio".to_owned(), "mentor".to_owned()];
        let mut resolved = HashMap::new();
        resolved.insert(ADA.to_owned(), secret);
        let mut doc = document(&json!({"author": ADA}));
        expand(&definition(), &mut doc, &resolved);
        let populated = doc["author"].as_object().expect("an object");
        assert_eq!(populated["hidden_fields"], json!(["bio", "mentor"]));
        assert_eq!(populated["content"], json!({"name": "Ada"}));
    }

    #[test]
    fn collect_ids_names_only_the_single_reference_fields() {
        let mut ids = BTreeSet::new();
        let doc = document(&json!({
            "author": ADA,
            "related": ["523e4567-e89b-12d3-a456-426614174004"],
            "meta": {"see": "623e4567-e89b-12d3-a456-426614174005"}
        }));
        collect_ids(&definition(), &doc, &mut ids);
        assert_eq!(ids.into_iter().collect::<Vec<_>>(), [ADA.to_owned()]);
    }

    #[test]
    fn an_expanded_document_collects_no_ids_of_its_own() {
        // A second `collect_ids` pass over a populated document finds the expanded
        // objects, not ids — nothing is double-counted across levels.
        let mut resolved = HashMap::new();
        resolved.insert(ADA.to_owned(), ada());
        let mut doc = document(&json!({"author": ADA}));
        expand(&definition(), &mut doc, &resolved);
        let mut ids = BTreeSet::new();
        collect_ids(&definition(), &doc, &mut ids);
        assert!(ids.is_empty());
    }
}
