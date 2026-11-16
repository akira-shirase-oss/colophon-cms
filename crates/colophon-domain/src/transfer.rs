//! What an import item is mapped from: a native content document (the shape `export` writes),
//! a Strapi entity or export, or a Sanity document (D-114). Pure JSON transforms, like the rest
//! of this crate: no I/O, no async, no database.
//!
//! [`map`] unwraps the source's envelope and hands back the document the collection's
//! definition validates by field name, plus the source metadata the per-item report keeps:
//! nothing the source said is silently dropped — metadata keys the mapping consumed but didn't
//! import are named in `unmapped`, and content keys the definition doesn't declare stay in the
//! document, where validation refuses them by name.

use serde_json::{Map, Value};

use crate::schema::{ContentType, FieldKind, Refusal};

/// The most marked envelopes (`{"type": …, "data": …}`) one item may nest.
const MAX_UNWRAP: usize = 8;

/// Strapi's bookkeeping keys beside or inside the content: REST system fields and the export's
/// database row columns. When they aren't declared fields they are set aside, named in
/// `unmapped`. `id`, `documentId`, `document_id`, `locale` and `localizations` are mapped or
/// reported separately, so they aren't here.
const STRAPI_META: &[&str] = &[
    "createdAt",
    "updatedAt",
    "publishedAt",
    "created_at",
    "updated_at",
    "published_at",
    "createdBy",
    "updatedBy",
    "publishedBy",
    "reviewedBy",
    "created_by_id",
    "updated_by_id",
    "published_by_id",
    "reviewed_by_id",
    "status",
];

/// One import item normalized: the document its collection validates, and the source metadata
/// kept for the report.
#[derive(Debug, Clone, PartialEq)]
pub struct Mapped {
    /// Fields by name, to validate against the collection's current definition. On a reference
    /// field, a `{"_ref": …}` (Sanity), `{"data": {"id": …}}` (Strapi v4 relation) or
    /// `{"documentId": …}` (Strapi v5 relation) value unwraps to the id it names.
    pub document: Value,
    /// The source's own name for the entry — a Sanity `_id`, a Strapi `id`/`documentId` —
    /// when it carried one the report can show.
    pub source: Option<String>,
    /// A Sanity `_type`: the source's name for the collection the document came from.
    pub kind: Option<String>,
    /// A Strapi `locale`, for the entry's tag (D-087).
    pub locale: Option<String>,
    /// Source keys set aside rather than imported, named for the report and sorted, so the
    /// order never depends on the map backend. Structural envelope keys (`type`, `data`,
    /// `version`, `attributes`) and keys kept elsewhere (`_id` → `source`, `_type` → `kind`,
    /// `locale`) aren't listed.
    pub unmapped: Vec<String>,
}

impl Mapped {
    /// A plain document: the item itself, nothing else known about it.
    fn plain(item: &Value) -> Self {
        Self {
            document: item.clone(),
            source: None,
            kind: None,
            locale: None,
            unmapped: Vec::new(),
        }
    }
}

/// A refusal inside one item, with `at` for its pointer.
fn refused(code: &'static str, at: &str, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        pointer: at.to_owned(),
        message: message.into(),
    }
}

/// Where an item's envelope unwraps to a `data` payload: a `{"type": "<ns>::<x>", "data": {…}}`
/// or `{"version": …, "data": {…}}` object (D-086). Content that merely has keys named `type`
/// or `data` is left alone: only a `::`-marked `type` or an explicit `version` marks one.
fn marked_data(item: &Value) -> Option<&Value> {
    let object = item.as_object()?;
    let data = object.get("data")?;
    let marked = object
        .get("type")
        .and_then(Value::as_str)
        .is_some_and(|kind| kind.contains("::"))
        || object.contains_key("version");
    marked.then_some(data)
}

/// `item` with every marked envelope unwrapped (at most [`MAX_UNWRAP`]), and whether any were.
fn unwrap(item: &Value) -> (&Value, bool) {
    let mut item = item;
    let mut unwrapped = false;
    for _ in 0..MAX_UNWRAP {
        let Some(data) = marked_data(item) else {
            break;
        };
        item = data;
        unwrapped = true;
    }
    (item, unwrapped)
}

/// A source id as text: a string as-is, a number written out. Anything else isn't an id.
fn source_id(value: &Value) -> Option<String> {
    if let Some(id) = value.as_str() {
        return Some(id.to_owned());
    }
    if let Some(id) = value.as_i64() {
        return Some(id.to_string());
    }
    value.as_f64().map(|id| id.to_string())
}

/// A non-string/number value where a source id belongs: `at` is the key it sits under.
fn bad_id(at: &str) -> Refusal {
    refused(
        "invalid_id",
        at,
        "a source id is a string or a number; this one is neither",
    )
}

/// `object`'s value at `key` as a source id — `None` if absent, a refusal if unshaped.
fn take_id(object: &Map<String, Value>, key: &str, mapped: &mut Mapped) -> Result<(), Refusal> {
    let Some(value) = object.get(key) else {
        return Ok(());
    };
    match source_id(value) {
        Some(id) => {
            mapped.source = Some(id);
            Ok(())
        }
        None => Err(bad_id(&format!("/{key}"))),
    }
}

/// A Sanity document: `_id` and `_type` map, every other top-level `_`-key is set aside, and
/// the rest are fields by name. Field ids can't start with `_` ([`is_field_id`]), so stripping
/// them can never eat a declared field.
///
/// [`is_field_id`]: crate::schema::is_field_id
fn sanity(object: &Map<String, Value>, mapped: &mut Mapped) -> Result<(), Refusal> {
    let mut document = Map::with_capacity(object.len());
    for (key, value) in object {
        match key.as_str() {
            "_id" => {
                mapped.source = Some(value.as_str().ok_or_else(|| bad_id("/_id"))?.to_owned());
            }
            "_type" => {
                mapped.kind = Some(
                    value
                        .as_str()
                        .ok_or_else(|| {
                            refused(
                                "invalid_type",
                                "/_type",
                                "a document's `_type` is a string naming its collection",
                            )
                        })?
                        .to_owned(),
                );
            }
            _ if key.starts_with('_') => mapped.unmapped.push(key.clone()),
            _ => {
                document.insert(key.clone(), value.clone());
            }
        }
    }
    mapped.document = Value::Object(document);
    Ok(())
}

/// An `{"id": …, "attributes": {…}}` entity's `attributes`, when that is what the object is:
/// `attributes` holds the fields (when it isn't one itself — a declared `attributes` field
/// means the item is plain content), and every other key is an envelope key.
fn v4_attributes<'a>(
    object: &'a Map<String, Value>,
    fields: &[String],
) -> Option<&'a Map<String, Value>> {
    let attributes = object.get("attributes")?.as_object()?;
    let v4 = !fields.iter().any(|field| field == "attributes")
        && object.keys().all(|key| {
            matches!(
                key.as_str(),
                "attributes" | "id" | "documentId" | "document_id" | "meta"
            )
        });
    v4.then_some(attributes)
}

/// A Strapi document's fields: id keys map to `source`, `locale` to `locale`, bookkeeping keys
/// to `unmapped`, and the rest stay by name. `fields` is the definition's declared ids, so a
/// key a schema actually uses is never eaten.
fn strapi_document(
    object: &Map<String, Value>,
    fields: &[String],
    mapped: &mut Mapped,
) -> Result<Value, Refusal> {
    let mut document = Map::with_capacity(object.len());
    for (key, value) in object {
        match key.as_str() {
            "id" | "documentId" | "document_id" if !fields.iter().any(|f| f == key) => {
                match source_id(value) {
                    Some(id) => mapped.source = Some(id),
                    None => return Err(bad_id(&format!("/{key}"))),
                }
            }
            "locale" if !fields.iter().any(|f| f == key) => match value.as_str() {
                Some(locale) => mapped.locale = Some(locale.to_owned()),
                None => mapped.unmapped.push(key.clone()),
            },
            _ if STRAPI_META.contains(&key.as_str()) && !fields.iter().any(|f| f == key) => {
                mapped.unmapped.push(key.clone());
            }
            "localizations" | "meta" if !fields.iter().any(|f| f == key) => {
                mapped.unmapped.push(key.clone());
            }
            _ => {
                document.insert(key.clone(), value.clone());
            }
        }
    }
    Ok(Value::Object(document))
}

/// One reference value's id, whatever shape it came in: a Sanity `{"_ref": "…"}` (with or
/// without its `_type`), a Strapi relation `{"data": {"id": …}}`, or a Strapi v5
/// `{"documentId": "…"}`. Anything else isn't translated.
fn reference_id(value: &Value) -> Option<String> {
    let object = value.as_object()?;
    if let Some(id) = object.get("_ref") {
        return source_id(id);
    }
    if let Some(data) = object.get("data")
        && let Some(id) = data.get("id").or_else(|| data.get("documentId"))
    {
        return source_id(id);
    }
    object.get("documentId").and_then(source_id)
}

/// On each of `content_type`'s top-level reference fields, unwraps foreign reference shapes to
/// the id they name — at the top level only; a `_ref` inside an object or block is the
/// definition's to refuse, never a guess to make.
fn translate_references(document: &mut Value, content_type: &ContentType) {
    let Some(object) = document.as_object_mut() else {
        return;
    };
    for field in content_type.fields() {
        let FieldKind::Reference { many, .. } = &field.kind else {
            continue;
        };
        let Some(value) = object.get_mut(&field.id) else {
            continue;
        };
        if *many {
            if let Value::Array(list) = value {
                for item in list.iter_mut() {
                    if let Some(id) = reference_id(item) {
                        *item = Value::String(id);
                    }
                }
            }
        } else if let Some(id) = reference_id(value) {
            *value = Value::String(id);
        }
    }
}

/// `item` as a content document for `content_type` plus its source metadata (D-114). Detection
/// is by shape, most specific first: marked envelopes unwrap, then `_`-keyed Sanity documents,
/// then `{id, attributes}` Strapi v4 entities, then flat documents carrying a source id key —
/// where "flat Strapi" only counts when the id key isn't a declared field (a schema always
/// beats a shape). Anything else is a native document, validated as written.
pub fn map(item: &Value, content_type: &ContentType) -> Result<Mapped, Refusal> {
    let (wrapped, unwrapped) = unwrap(item);
    let mut mapped = Mapped::plain(wrapped);
    let Some(object) = wrapped.as_object() else {
        return Ok(mapped);
    };
    let fields: Vec<String> = content_type
        .fields()
        .iter()
        .map(|field| field.id.clone())
        .collect();
    if object.contains_key("_id") || object.contains_key("_type") {
        sanity(object, &mut mapped)?;
    } else if let Some(attributes) = v4_attributes(object, &fields) {
        take_id(object, "id", &mut mapped)?;
        take_id(object, "documentId", &mut mapped)?;
        take_id(object, "document_id", &mut mapped)?;
        if object.contains_key("meta") {
            mapped.unmapped.push("meta".to_owned());
        }
        mapped.document = strapi_document(attributes, &fields, &mut mapped)?;
    } else if unwrapped
        || ["id", "documentId", "document_id"]
            .iter()
            .any(|key| object.contains_key(*key) && !fields.iter().any(|f| f == key))
    {
        // A marked envelope's flat `data`, or a flat document carrying a source id key the
        // schema doesn't declare: the Strapi flat shape (v5 REST, or an export row).
        mapped.document = strapi_document(object, &fields, &mut mapped)?;
    }
    translate_references(&mut mapped.document, content_type);
    // The report's set-aside list is canonical whatever serde_json's map backend preserves.
    mapped.unmapped.sort();
    Ok(mapped)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn notes() -> ContentType {
        ContentType::parse(&json!({
            "name": "Notes", "title_field": "title", "fields": [
                {"id": "title", "label": "Title", "type": "text", "required": true},
                {"id": "note", "label": "Note", "type": "text"},
                {"id": "writer", "label": "Writer", "type": "reference", "to": "writers"},
                {"id": "tags", "label": "Tags", "type": "reference", "to": "tags", "many": true, "max": 5}
            ]
        }))
        .expect("a definition")
    }

    #[test]
    fn a_plain_document_passes_through() {
        let item =
            json!({"title": "Hi", "note": "a field literally called data next", "data": "x"});
        let mapped = map(&item, &notes()).expect("maps");
        assert_eq!(mapped.document, item);
        assert_eq!(mapped.source, None);
        assert_eq!(mapped.kind, None);
        assert_eq!(mapped.unmapped, Vec::<String>::new());
    }

    #[test]
    fn a_marked_envelope_unwraps_and_flattens() {
        // The transfer entities D-086 already took: `data` holds the fields.
        let item =
            json!({"type": "api::note.note", "data": {"title": "Hi", "createdAt": "2026-01-01"}});
        let mapped = map(&item, &notes()).expect("maps");
        assert_eq!(mapped.document, json!({"title": "Hi"}));
        assert_eq!(mapped.unmapped, ["createdAt"]);
        let marker = json!({"version": 2, "data": {"title": "Hi"}});
        assert_eq!(
            map(&marker, &notes()).expect("maps").document,
            json!({"title": "Hi"})
        );
        // Nested envelopes unwrap too.
        let nested =
            json!({"version": 1, "data": {"type": "api::note.note", "data": {"title": "Deep"}}});
        assert_eq!(
            map(&nested, &notes()).expect("maps").document,
            json!({"title": "Deep"})
        );
        // A `type` that isn't a Strapi namespace marks nothing: the item is plain content.
        let unmarked = json!({"type": "note", "data": {"title": "Hi"}, "title": "Mine"});
        assert_eq!(map(&unmarked, &notes()).expect("maps").document, unmarked);
    }

    #[test]
    fn a_strapi_v4_entity_maps_attributes_and_keeps_its_meta() {
        let item = json!({
            "id": 42,
            "attributes": {
                "title": "Hi", "note": "from v4",
                "createdAt": "2026-01-01T00:00:00Z", "updatedAt": "2026-01-01T00:00:00Z",
                "publishedAt": null, "locale": "en", "localizations": [{"id": 7}]
            }
        });
        let mapped = map(&item, &notes()).expect("maps");
        assert_eq!(mapped.document, json!({"title": "Hi", "note": "from v4"}));
        assert_eq!(mapped.source.as_deref(), Some("42"));
        assert_eq!(mapped.locale.as_deref(), Some("en"));
        assert_eq!(
            mapped.unmapped,
            ["createdAt", "localizations", "publishedAt", "updatedAt"]
        );
    }

    #[test]
    fn a_strapi_v5_flat_document_maps_document_id() {
        let item = json!({
            "documentId": "a1b2c3", "title": "Hi", "createdAt": "x", "status": "published",
            "locale": "fr"
        });
        let mapped = map(&item, &notes()).expect("maps");
        assert_eq!(mapped.document, json!({"title": "Hi"}));
        assert_eq!(mapped.source.as_deref(), Some("a1b2c3"));
        assert_eq!(mapped.locale.as_deref(), Some("fr"));
        assert_eq!(mapped.unmapped, ["createdAt", "status"]);
    }

    #[test]
    fn a_schema_field_named_id_keeps_the_document_native() {
        // `id` is a legal field id, so a declared one beats the Strapi flat shape — the
        // document is content, validated as written.
        let type_with_id = ContentType::parse(&json!({
            "name": "D", "title_field": "title", "fields": [
                {"id": "title", "label": "Title", "type": "text", "required": true},
                {"id": "id", "label": "Identifier", "type": "text"}
            ]
        }))
        .expect("a definition");
        let item = json!({"id": "a1b2c3", "title": "Hi"});
        let mapped = map(&item, &type_with_id).expect("maps");
        assert_eq!(mapped.document, item);
        assert_eq!(mapped.source, None);
    }

    #[test]
    fn a_sanity_document_maps_its_system_keys_and_type() {
        let item = json!({
            "_id": "drafts.7f1c", "_type": "note", "_rev": "r3", "_createdAt": "x",
            "title": "Hi", "note": "sanity"
        });
        let mapped = map(&item, &notes()).expect("maps");
        assert_eq!(mapped.document, json!({"title": "Hi", "note": "sanity"}));
        assert_eq!(mapped.source.as_deref(), Some("drafts.7f1c"));
        assert_eq!(mapped.kind.as_deref(), Some("note"));
        assert_eq!(mapped.unmapped, ["_createdAt", "_rev"]);
        // A non-string `_id` or `_type` refuses rather than guessing.
        assert_eq!(
            map(&json!({"_id": 4, "title": "x"}), &notes())
                .unwrap_err()
                .code,
            "invalid_id"
        );
        assert_eq!(
            map(&json!({"_type": 4}), &notes()).unwrap_err().code,
            "invalid_type"
        );
    }

    #[test]
    fn foreign_reference_shapes_unwrap_on_declared_reference_fields() {
        let item = json!({
            "_id": "x", "_type": "note", "title": "Hi",
            "writer": {"_type": "reference", "_ref": "ada"},
            "tags": [{"_ref": "t1"}, {"data": {"id": 9}}]
        });
        let mapped = map(&item, &notes()).expect("maps");
        assert_eq!(mapped.document["writer"], "ada");
        assert_eq!(mapped.document["tags"], json!(["t1", "9"]));
        // A Strapi `{"data": {"id": …}}` relation unwraps the same way.
        let item =
            json!({"id": 1, "title": "Hi", "writer": {"data": {"id": 42, "attributes": {}}}});
        assert_eq!(map(&item, &notes()).expect("maps").document["writer"], "42");
    }

    #[test]
    fn ids_we_cannot_read_are_refused_not_dropped() {
        let err = map(&json!({"id": [1, 2], "title": "x"}), &notes()).unwrap_err();
        assert_eq!((err.code, err.pointer.as_str()), ("invalid_id", "/id"));
    }
}
