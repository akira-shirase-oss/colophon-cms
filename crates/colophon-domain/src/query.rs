//! The bounded query language: one JSON document that filters, sorts, selects and pages
//! a collection's entries (docs/modules/01-domain.md, "The query language"; milestone D;
//! D-103).
//!
//! A query is a document sent in a `POST` body, like a definition or a changeset, not a spray
//! of URL parameters: one parser, one canonical form, one SHA-256. [`Query::validate`]
//! checks a document against a [`ContentType`] — every named field exists, is `index`ed
//! for its use, and every compared value is one the field itself accepts — and refuses
//! with a [`Refusal`] and a JSON Pointer like the definition's own. The canonical form
//! and [`Query::query_hash`] then name the exact query: a cache key, the shape a cursor
//! is bound to. [`Query::order`] is the ordering a page runs under, the entry's id last
//! as the tie-break, so equal sort values still give every page boundary one place.
//!
//! Everything is bounded: at most [`MAX_FILTERS`] filters, [`MAX_SORT_KEYS`] sort keys,
//! [`MAX_IN_VALUES`] values to an `in`, a page of [`MAX_LIMIT`] entries and an expansion
//! depth of [`MAX_DEPTH`] (the spec's relation-expansion bound).

use serde_json::{Map, Value};
use sha2::{Digest, Sha256};

use crate::{
    rich_text::{json::write_string, pointer::Pointer},
    schema::{ContentType, Field, FieldKind, MAX_FIELDS, Refusal},
};

/// The most filters one query may hold.
pub const MAX_FILTERS: usize = 8;
/// The most sort keys one query may hold.
pub const MAX_SORT_KEYS: usize = 4;
/// The most values an `in` filter compares against.
pub const MAX_IN_VALUES: usize = 100;
/// The page size a query takes when it doesn't say.
pub const DEFAULT_LIMIT: u16 = 20;
/// The most entries one page may hold (the delivery API's page bound).
pub const MAX_LIMIT: u16 = 100;
/// The deepest references expand: the spec's relation-expansion bound.
pub const MAX_DEPTH: u8 = 2;

/// How a filter compares the field's stored values with the document's `value`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Operator {
    /// Equal.
    Eq,
    /// One of a list of distinct values.
    In,
    /// Less than (ordered kinds only).
    Lt,
    /// Less than or equal (ordered kinds only).
    Lte,
    /// Greater than (ordered kinds only).
    Gt,
    /// Greater than or equal (ordered kinds only).
    Gte,
    /// Holds the value as a substring (single-line text only).
    Contains,
}

impl Operator {
    /// The name in a query document.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Eq => "eq",
            Self::In => "in",
            Self::Lt => "lt",
            Self::Lte => "lte",
            Self::Gt => "gt",
            Self::Gte => "gte",
            Self::Contains => "contains",
        }
    }
}

/// A sort key's direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    /// Smallest first (`"asc"`, the default).
    Asc,
    /// Largest first (`"desc"`).
    Desc,
}

impl Direction {
    /// The name in a query document.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        }
    }
}

/// One comparison: `field`'s stored values against `value` under `op`.
#[derive(Debug, Clone, PartialEq)]
pub struct Filter {
    /// The field's id: a top-level, `index`ed field of the content type.
    pub field: String,
    /// The comparison it makes.
    pub op: Operator,
    /// The compared value in canonical JSON — the exact spelling stored content uses, so
    /// a string is quoted, a number has one spelling and `in` a sorted list. It parses
    /// with `serde_json::from_str`.
    pub value: String,
}

/// One sort key as the document declared it.
#[derive(Debug, Clone, PartialEq)]
pub struct Sort {
    /// The field's id: a top-level, `index`ed field of the content type.
    pub field: String,
    /// The direction.
    pub dir: Direction,
}

/// One key of the ordering a page runs under ([`Query::order`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OrderKey {
    /// A declared field's value.
    Field(String),
    /// The entry's id: the tie-break every query ends on, so equal sort values still
    /// give every page boundary one place.
    EntryId,
}

/// A validated query: filters, sort, page size, field selection and expansion depth,
/// checked against one content type.
#[derive(Debug, Clone, PartialEq)]
pub struct Query {
    filters: Vec<Filter>,
    sort: Vec<Sort>,
    limit: u16,
    fields: Option<Vec<String>>,
    depth: u8,
}

impl Query {
    /// Parses and checks `value` against `definition`: a JSON object holding `filters`,
    /// `sort`, `limit`, `fields` and `depth`, and nothing else. Every refusal is a
    /// [`Refusal`] with a JSON Pointer into the document.
    ///
    /// ```json
    /// {"filters": [{"field": "author", "op": "eq", "value": "…"}],
    ///  "sort": [{"field": "published_at", "dir": "desc"}],
    ///  "limit": 50, "fields": ["title", "slug"], "depth": 1}
    /// ```
    pub fn validate(definition: &ContentType, value: &Value) -> Result<Self, Refusal> {
        let root = Pointer::new();
        let object = value
            .as_object()
            .ok_or_else(|| refused("not_an_object", &root, "a query is a JSON object"))?;
        only(
            object,
            &["filters", "sort", "limit", "fields", "depth"],
            &root,
        )?;
        let filters = match object.get("filters") {
            None => Vec::new(),
            Some(value) => filters(definition, value, &at(&root, "filters"))?,
        };
        let sort = match object.get("sort") {
            None => Vec::new(),
            Some(value) => sort(definition, value, &at(&root, "sort"))?,
        };
        let limit = match object.get("limit") {
            None => DEFAULT_LIMIT,
            Some(value) => whole(value, "limit", 1, MAX_LIMIT, &at(&root, "limit"))?,
        };
        let depth = match object.get("depth") {
            None => 0,
            // `whole` already bounded it to `MAX_DEPTH`.
            Some(value) => u8::try_from(whole(
                value,
                "depth",
                0,
                u16::from(MAX_DEPTH),
                &at(&root, "depth"),
            )?)
            .unwrap_or(MAX_DEPTH),
        };
        let fields = match object.get("fields") {
            None => None,
            Some(value) => Some(selection(definition, value, &at(&root, "fields"))?),
        };
        Ok(Self {
            filters,
            sort,
            limit,
            fields,
            depth,
        })
    }

    /// The filters, in canonical order: filters AND together, so each is sorted by its
    /// own canonical text and one set of filters has one spelling.
    pub fn filters(&self) -> &[Filter] {
        &self.filters
    }

    /// The declared sort keys, in their order — the order is significant.
    pub fn sort(&self) -> &[Sort] {
        &self.sort
    }

    /// The page size: [`DEFAULT_LIMIT`] when the query didn't say, never more than
    /// [`MAX_LIMIT`].
    pub fn limit(&self) -> u16 {
        self.limit
    }

    /// The field selection: `None` keeps the whole document; `Some` only narrows it, so
    /// it carries no permission risk, in definition order.
    pub fn fields(&self) -> Option<&[String]> {
        self.fields.as_deref()
    }

    /// How deep references expand: 0 (the default) leaves entry ids; [`MAX_DEPTH`] is the
    /// bound.
    pub fn depth(&self) -> u8 {
        self.depth
    }

    /// The ordering a page runs under: the declared keys, then the entry's id in the last
    /// key's direction (`asc` when nothing is sorted), the tie-break.
    pub fn order(&self) -> Vec<(OrderKey, Direction)> {
        let mut order: Vec<(OrderKey, Direction)> = self
            .sort
            .iter()
            .map(|key| (OrderKey::Field(key.field.clone()), key.dir))
            .collect();
        let tie = self.sort.last().map_or(Direction::Asc, |key| key.dir);
        order.push((OrderKey::EntryId, tie));
        order
    }

    /// The document in its one spelling: keys sorted, every default written out, filters
    /// and `in` values in canonical order and values in canonical JSON, `fields` `null`
    /// when the whole document is kept. Two spellings of the same query have the same
    /// bytes — and the same hash.
    pub fn canonical_json(&self) -> String {
        let mut out = String::with_capacity(128);
        out.push_str(r#"{"depth":"#);
        out.push_str(&self.depth.to_string());
        out.push_str(r#","fields":"#);
        match &self.fields {
            None => out.push_str("null"),
            Some(fields) => {
                out.push('[');
                for (index, field) in fields.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(&mut out, field);
                }
                out.push(']');
            }
        }
        out.push_str(r#","filters":["#);
        for (index, filter) in self.filters.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(&canonical_filter(filter));
        }
        out.push_str(r#"],"limit":"#);
        out.push_str(&self.limit.to_string());
        out.push_str(r#","sort":["#);
        for (index, key) in self.sort.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_sort(&mut out, key);
        }
        out.push_str("]}");
        out
    }

    /// SHA-256 of `canonical_json` (D-029): what a cache key or a cursor's query binding
    /// names.
    pub fn query_hash(&self) -> [u8; 32] {
        let digest = Sha256::digest(self.canonical_json().as_bytes());
        let mut out = [0_u8; 32];
        out.copy_from_slice(&digest);
        out
    }

    /// `query_hash` as 64 lowercase hex digits, for cache keys.
    pub fn query_hash_hex(&self) -> String {
        const HEX: &[u8; 16] = b"0123456789abcdef";
        let mut text = String::with_capacity(64);
        for byte in self.query_hash() {
            text.push(char::from(HEX[usize::from(byte >> 4)]));
            text.push(char::from(HEX[usize::from(byte & 0xf)]));
        }
        text
    }
}

/// A refusal inside a query document, with `pointer` for its place in it.
fn refused(code: &'static str, pointer: &Pointer, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        pointer: pointer.as_str().to_owned(),
        message: message.into(),
    }
}

/// `pointer` with one more segment.
fn at(pointer: &Pointer, key: &str) -> Pointer {
    let mut next = pointer.clone();
    next.push(key);
    next
}

/// Refuses any key of `object` outside `allowed`, like the definitions' rule: nothing is
/// dropped silently (D11).
fn only(object: &Map<String, Value>, allowed: &[&str], pointer: &Pointer) -> Result<(), Refusal> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => {
            let shown: String = key.chars().take(64).collect();
            Err(refused(
                "unknown_field",
                &at(pointer, &shown),
                format!("`{shown}` is not allowed here"),
            ))
        }
        None => Ok(()),
    }
}

/// The value `key` holds in `object`, or a `missing_field` refusal pointing where it goes.
fn require<'a>(
    object: &'a Map<String, Value>,
    key: &str,
    pointer: &Pointer,
) -> Result<&'a Value, Refusal> {
    object.get(key).ok_or_else(|| {
        refused(
            "missing_field",
            &at(pointer, key),
            format!("`{key}` is required"),
        )
    })
}

/// A whole number in `low..=high`, for `limit` and `depth`.
fn whole(value: &Value, key: &str, low: u16, high: u16, pointer: &Pointer) -> Result<u16, Refusal> {
    value
        .as_u64()
        .and_then(|number| u16::try_from(number).ok())
        .filter(|number| (low..=high).contains(number))
        .ok_or_else(|| {
            refused(
                "out_of_range",
                pointer,
                format!("`{key}` is a whole number from {low} to {high}"),
            )
        })
}

/// The field `value` names: a top-level field id the content type declares.
fn declared<'a>(
    definition: &'a ContentType,
    value: &Value,
    pointer: &Pointer,
) -> Result<&'a Field, Refusal> {
    let id = value
        .as_str()
        .ok_or_else(|| refused("wrong_type", pointer, "a field is named by its id"))?;
    definition
        .fields()
        .iter()
        .find(|field| field.id.as_str() == id)
        .ok_or_else(|| {
            refused(
                "unknown_field",
                pointer,
                format!("`{id}` isn't a field of this content type"),
            )
        })
}

/// The operators a kind takes (D-103): `eq` and `in` on every indexed kind, ordering on
/// kinds whose values order as written, `contains` on single-line text only — where a
/// substring search reads naturally. Boolean, choice, email, URL and single reference
/// stay at equality: ordering on them would compare a spelling, not a meaning.
fn ops_for(kind: &FieldKind) -> &'static [Operator] {
    use Operator::{Contains, Eq, Gt, Gte, In, Lt, Lte};
    const EQUALITY: &[Operator] = &[Eq, In];
    const ORDERED: &[Operator] = &[Eq, In, Lt, Lte, Gt, Gte];
    const TEXT: &[Operator] = &[Eq, In, Lt, Lte, Gt, Gte, Contains];
    match kind {
        // Only single-line text is indexable, so `contains` is safe to offer here.
        FieldKind::Text { .. } => TEXT,
        FieldKind::Number { .. }
        | FieldKind::DateTime
        | FieldKind::Date
        | FieldKind::Time
        | FieldKind::Slug { .. } => ORDERED,
        // A kind that isn't indexable never reaches here: `field.index` is checked first.
        _ => EQUALITY,
    }
}

/// The filter list: at most [`MAX_FILTERS`], each a `{"field", "op", "value"}` object,
/// sorted by canonical text so one query has one spelling.
fn filters(
    definition: &ContentType,
    value: &Value,
    pointer: &Pointer,
) -> Result<Vec<Filter>, Refusal> {
    let list = value.as_array().ok_or_else(|| {
        refused(
            "wrong_type",
            pointer,
            "`filters` is a list of {\"field\", \"op\", \"value\"}",
        )
    })?;
    if list.len() > MAX_FILTERS {
        return Err(refused(
            "too_many",
            pointer,
            format!("a query filters at most {MAX_FILTERS} times"),
        ));
    }
    let mut filters = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let mut at_item = pointer.clone();
        at_item.push_index(index);
        filters.push(filter(definition, item, &at_item)?);
    }
    filters.sort_by_cached_key(canonical_filter);
    Ok(filters)
}

/// One filter: `field` declared and `index`ed, `op` one the field's kind takes, `value`
/// checked by the field's own rules and kept in canonical JSON.
fn filter(definition: &ContentType, value: &Value, pointer: &Pointer) -> Result<Filter, Refusal> {
    let object = value.as_object().ok_or_else(|| {
        refused(
            "not_an_object",
            pointer,
            "a filter is {\"field\", \"op\", \"value\"}",
        )
    })?;
    only(object, &["field", "op", "value"], pointer)?;
    let at_field = at(pointer, "field");
    let field = declared(definition, require(object, "field", pointer)?, &at_field)?;
    if !field.index {
        return Err(refused(
            "field_not_indexed",
            &at_field,
            format!("`{}` isn't declared `index`ed", field.id),
        ));
    }
    let op = operator(require(object, "op", pointer)?, field, &at(pointer, "op"))?;
    let value = compared(
        field,
        op,
        require(object, "value", pointer)?,
        &at(pointer, "value"),
    )?;
    Ok(Filter {
        field: field.id.clone(),
        op,
        value,
    })
}

/// The `op` of a filter: a name it knows, and one `field`'s kind takes.
fn operator(value: &Value, field: &Field, pointer: &Pointer) -> Result<Operator, Refusal> {
    let name = value.as_str().ok_or_else(|| {
        refused(
            "invalid_op",
            pointer,
            "`op` is one of eq, in, lt, lte, gt, gte, contains",
        )
    })?;
    let op = match name {
        "eq" => Operator::Eq,
        "in" => Operator::In,
        "lt" => Operator::Lt,
        "lte" => Operator::Lte,
        "gt" => Operator::Gt,
        "gte" => Operator::Gte,
        "contains" => Operator::Contains,
        _ => {
            return Err(refused(
                "invalid_op",
                pointer,
                format!("`{name}` isn't an operator; use eq, in, lt, lte, gt, gte or contains"),
            ));
        }
    };
    let ops = ops_for(&field.kind);
    if !ops.contains(&op) {
        let names: Vec<&str> = ops.iter().map(Operator::name).collect();
        return Err(refused(
            "invalid_op",
            pointer,
            format!(
                "`{}` takes {}; it doesn't take `{}`",
                field.label,
                names.join(", "),
                op.name()
            ),
        ));
    }
    Ok(op)
}

/// The value a filter compares against, in canonical JSON: for `in`, a list of 1 to
/// [`MAX_IN_VALUES`] different ones; for `contains`, a needle; for the rest, one value
/// the field accepts.
fn compared(
    field: &Field,
    op: Operator,
    value: &Value,
    pointer: &Pointer,
) -> Result<String, Refusal> {
    match op {
        Operator::In => {
            let list = value.as_array().ok_or_else(|| {
                refused(
                    "wrong_type",
                    pointer,
                    "`in` compares against a list of values",
                )
            })?;
            if list.is_empty() || list.len() > MAX_IN_VALUES {
                return Err(refused(
                    "invalid_value",
                    pointer,
                    format!("`in` compares against 1 to {MAX_IN_VALUES} different values"),
                ));
            }
            let mut values = Vec::with_capacity(list.len());
            for (index, item) in list.iter().enumerate() {
                let mut at_item = pointer.clone();
                at_item.push_index(index);
                values.push(literal(field, item, &at_item)?);
            }
            // `in` names a set: the canonical order is by each value's own text.
            values.sort();
            if values.windows(2).any(|pair| pair[0] == pair[1]) {
                return Err(refused(
                    "duplicate_value",
                    pointer,
                    "`in` compares against different values",
                ));
            }
            Ok(format!("[{}]", values.join(",")))
        }
        Operator::Contains => needle(field, value, pointer),
        _ => literal(field, value, pointer),
    }
}

/// `value` checked by `field`'s own rules and written canonically — a filter can never
/// compare against something the field couldn't hold.
fn literal(field: &Field, value: &Value, pointer: &Pointer) -> Result<String, Refusal> {
    let mut out = String::new();
    ContentType::literal(field, value, pointer, &mut out).map_err(|refusal| {
        refused(
            "invalid_value",
            pointer,
            format!(
                "the value isn't one `{}` accepts: {}",
                field.label, refusal.message
            ),
        )
    })?;
    Ok(out)
}

/// A `contains` needle: non-empty, without control characters (the field's values have
/// none) and no longer than the field allows — a longer one can't match anything it
/// holds. Unlike a compared value it needn't satisfy the field's other rules: it
/// searches inside them.
fn needle(field: &Field, value: &Value, pointer: &Pointer) -> Result<String, Refusal> {
    let FieldKind::Text { max_chars, .. } = field.kind else {
        // `ops_for` offers `contains` on text fields only.
        return Err(refused(
            "invalid_op",
            pointer,
            "`contains` is for single-line text",
        ));
    };
    let text = value
        .as_str()
        .ok_or_else(|| refused("wrong_type", pointer, "`contains` takes text"))?;
    let length = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
    if text.is_empty() || length > max_chars || text.chars().any(char::is_control) {
        return Err(refused(
            "invalid_value",
            pointer,
            format!("`contains` takes 1 to {max_chars} characters without control characters"),
        ));
    }
    let mut out = String::new();
    write_string(&mut out, text);
    Ok(out)
}

/// The sort list: at most [`MAX_SORT_KEYS`] `{"field", "dir"}` objects on declared,
/// `index`ed fields, no field twice; `dir` defaults to `"asc"`.
fn sort(definition: &ContentType, value: &Value, pointer: &Pointer) -> Result<Vec<Sort>, Refusal> {
    let list = value.as_array().ok_or_else(|| {
        refused(
            "wrong_type",
            pointer,
            "`sort` is a list of {\"field\", \"dir\"}",
        )
    })?;
    if list.len() > MAX_SORT_KEYS {
        return Err(refused(
            "too_many",
            pointer,
            format!("a query sorts on at most {MAX_SORT_KEYS} fields"),
        ));
    }
    let mut sort: Vec<Sort> = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let mut at_item = pointer.clone();
        at_item.push_index(index);
        let object = item.as_object().ok_or_else(|| {
            refused(
                "not_an_object",
                &at_item,
                "a sort key is {\"field\", \"dir\"}",
            )
        })?;
        only(object, &["field", "dir"], &at_item)?;
        let at_field = at(&at_item, "field");
        let field = declared(definition, require(object, "field", &at_item)?, &at_field)?;
        if !field.index {
            return Err(refused(
                "field_not_indexed",
                &at_field,
                format!("`{}` isn't declared `index`ed", field.id),
            ));
        }
        if sort.iter().any(|seen| seen.field == field.id) {
            return Err(refused(
                "duplicate_field",
                &at_field,
                format!("`{}` sorts twice", field.id),
            ));
        }
        let dir = match object.get("dir") {
            None => Direction::Asc,
            Some(value) => match value.as_str() {
                Some("asc") => Direction::Asc,
                Some("desc") => Direction::Desc,
                _ => {
                    return Err(refused(
                        "invalid_dir",
                        &at(&at_item, "dir"),
                        "`dir` is \"asc\" or \"desc\"",
                    ));
                }
            },
        };
        sort.push(Sort {
            field: field.id.clone(),
            dir,
        });
    }
    Ok(sort)
}

/// The `fields` selection: 1 to [`MAX_FIELDS`] declared field ids, none twice, kept in
/// definition order — the order stored content writes them.
fn selection(
    definition: &ContentType,
    value: &Value,
    pointer: &Pointer,
) -> Result<Vec<String>, Refusal> {
    let list = value
        .as_array()
        .ok_or_else(|| refused("wrong_type", pointer, "`fields` is a list of field ids"))?;
    if list.is_empty() || list.len() > MAX_FIELDS {
        return Err(refused(
            "invalid_fields",
            pointer,
            format!("`fields` lists 1 to {MAX_FIELDS} fields, or is left out"),
        ));
    }
    let mut selected = Vec::with_capacity(list.len());
    for (index, item) in list.iter().enumerate() {
        let mut at_item = pointer.clone();
        at_item.push_index(index);
        let field = declared(definition, item, &at_item)?;
        if selected.contains(&field.id) {
            return Err(refused(
                "duplicate_field",
                &at_item,
                format!("`{}` is selected twice", field.id),
            ));
        }
        selected.push(field.id.clone());
    }
    Ok(definition
        .fields()
        .iter()
        .filter(|field| selected.contains(&field.id))
        .map(|field| field.id.clone())
        .collect())
}

/// A filter as the canonical document writes it: `{"field","op","value"}`, keys sorted.
fn canonical_filter(filter: &Filter) -> String {
    let mut out = String::with_capacity(64);
    out.push_str(r#"{"field":"#);
    write_string(&mut out, &filter.field);
    out.push_str(r#","op":"#);
    write_string(&mut out, filter.op.name());
    out.push_str(r#","value":"#);
    out.push_str(&filter.value);
    out.push('}');
    out
}

/// A sort key as the canonical document writes it: `{"dir","field"}`, keys sorted.
fn write_sort(out: &mut String, key: &Sort) {
    out.push_str(r#"{"dir":"#);
    write_string(out, key.dir.name());
    out.push_str(r#","field":"#);
    write_string(out, &key.field);
    out.push('}');
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    /// Every indexed kind on one type, plus declared fields that aren't indexed.
    fn post_type() -> ContentType {
        ContentType::parse(&json!({
            "name": "Posts",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120, "index": true},
                {"id": "author", "label": "Author", "type": "reference", "to": "staff", "index": true},
                {"id": "publish_at", "label": "Publish at", "type": "datetime", "index": true},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "index": true},
                {"id": "rating", "label": "Rating", "type": "number", "integer": true, "min": 0, "max": 5, "index": true},
                {"id": "score", "label": "Score", "type": "number", "index": true},
                {"id": "featured", "label": "Featured", "type": "boolean", "index": true},
                {"id": "slug", "label": "Slug", "type": "slug", "index": true},
                {"id": "email", "label": "Email", "type": "email", "index": true},
                {"id": "site", "label": "Site", "type": "url", "index": true},
                {"id": "born", "label": "Born", "type": "date", "index": true},
                {"id": "opens", "label": "Opens", "type": "time", "index": true},
                {"id": "plain", "label": "Plain", "type": "text"},
                {"id": "notes", "label": "Notes", "type": "text", "multiline": true},
                {"id": "body", "label": "Body", "type": "rich_text"},
                {"id": "meta", "label": "Meta", "type": "object", "fields": [
                    {"id": "seo", "label": "SEO", "type": "text"}
                ]},
                {"id": "tags", "label": "Tags", "type": "list", "of": {"type": "text"}},
                {"id": "related", "label": "Related", "type": "reference", "to": "articles", "many": true},
                {"id": "photo", "label": "Photo", "type": "asset"},
                {"id": "accent", "label": "Accent", "type": "color"},
                {"id": "scratch", "label": "Scratch", "type": "json"},
                {"id": "script", "label": "Script", "type": "markdown"}
            ]
        }))
        .expect("a valid definition")
    }

    const ENTRY: &str = "123e4567-e89b-12d3-a456-426614174000";
    const OTHER: &str = "123e4567-e89b-12d3-a456-426614174001";

    fn checked(value: &Value) -> Result<Query, Refusal> {
        Query::validate(&post_type(), value)
    }

    /// The refusal `value` gets, as its code and pointer, for table-driven checks.
    fn refusal_of(value: &Value) -> (&'static str, String) {
        let refusal = checked(value).expect_err("refused");
        (refusal.code, refusal.pointer)
    }

    #[test]
    fn a_query_filters_and_sorts_on_indexed_fields() {
        let query = checked(&json!({
            "filters": [
                {"field": "author", "op": "eq", "value": ENTRY},
                {"field": "section", "op": "in", "value": ["News", "Guides"]},
                {"field": "publish_at", "op": "lt", "value": "2026-10-25T14:30:00Z"},
                {"field": "publish_at", "op": "gte", "value": "2026-01-01T00:00:00Z"},
                {"field": "rating", "op": "gt", "value": 3},
                {"field": "score", "op": "lte", "value": 4.5},
                {"field": "featured", "op": "eq", "value": true},
                {"field": "headline", "op": "contains", "value": "colophon"}
            ],
            "sort": [{"field": "publish_at", "dir": "desc"}, {"field": "rating"}],
            "limit": 50,
            "fields": ["slug", "headline"],
            "depth": 2
        }))
        .expect("a valid query");
        let filters = query.filters();
        assert_eq!(filters.len(), 8);
        // Filters are stored in canonical order, not the document's.
        assert_eq!(filters[0].field, "author");
        assert_eq!(filters[0].value, format!("\"{ENTRY}\""));
        assert_eq!(filters[7].field, "section");
        assert_eq!(filters[7].value, "[\"Guides\",\"News\"]");
        assert_eq!(query.limit(), 50);
        assert_eq!(query.depth(), 2);
        // The selection is kept in definition order: headline before slug.
        let expected = vec!["headline".to_owned(), "slug".to_owned()];
        assert_eq!(query.fields(), Some(expected.as_slice()));
        assert_eq!(query.sort()[1].dir, Direction::Asc);
        assert_eq!(
            query.order(),
            vec![
                (OrderKey::Field("publish_at".to_owned()), Direction::Desc),
                (OrderKey::Field("rating".to_owned()), Direction::Asc),
                (OrderKey::EntryId, Direction::Asc),
            ]
        );
    }

    #[test]
    fn every_indexed_kind_takes_the_values_it_writes() {
        let cases = [
            (
                json!({"field": "headline", "op": "eq", "value": "Hi"}),
                "\"Hi\"",
            ),
            (
                json!({"field": "headline", "op": "lt", "value": "M"}),
                "\"M\"",
            ),
            (
                json!({"field": "headline", "op": "contains", "value": " the "}),
                "\" the \"",
            ),
            (
                json!({"field": "author", "op": "eq", "value": ENTRY}),
                "\"123e4567-e89b-12d3-a456-426614174000\"",
            ),
            (
                json!({"field": "author", "op": "in", "value": [OTHER, ENTRY]}),
                "[\"123e4567-e89b-12d3-a456-426614174000\",\"123e4567-e89b-12d3-a456-426614174001\"]",
            ),
            (
                json!({"field": "publish_at", "op": "eq", "value": "2026-10-25T14:30:00Z"}),
                "\"2026-10-25T14:30:00Z\"",
            ),
            (
                json!({"field": "section", "op": "eq", "value": "News"}),
                "\"News\"",
            ),
            (json!({"field": "rating", "op": "eq", "value": 5}), "5"),
            (json!({"field": "score", "op": "eq", "value": 4.5}), "4.5"),
            (
                json!({"field": "featured", "op": "eq", "value": false}),
                "false",
            ),
            (
                json!({"field": "featured", "op": "in", "value": [true, false]}),
                "[false,true]",
            ),
            (
                json!({"field": "slug", "op": "eq", "value": "my-post"}),
                "\"my-post\"",
            ),
            (
                json!({"field": "email", "op": "eq", "value": "dev@example.com"}),
                "\"dev@example.com\"",
            ),
            (
                json!({"field": "site", "op": "eq", "value": "https://example.com"}),
                // Canonicalized, as stored content is: the WHATWG form gains the `/`.
                "\"https://example.com/\"",
            ),
            (
                json!({"field": "born", "op": "gte", "value": "2000-02-29"}),
                "\"2000-02-29\"",
            ),
            (
                json!({"field": "opens", "op": "lt", "value": "14:30"}),
                "\"14:30\"",
            ),
        ];
        for (filter, canonical) in cases {
            let query = checked(&json!({"filters": [filter]})).expect("valid filter");
            assert_eq!(query.filters()[0].value, canonical, "{filter}");
        }
    }

    #[test]
    fn defaults_fill_limit_depth_and_selection() {
        let query = checked(&json!({})).expect("an empty object is a valid query");
        assert_eq!(query.limit(), DEFAULT_LIMIT);
        assert_eq!(query.depth(), 0);
        assert_eq!(query.fields(), None);
        assert!(query.filters().is_empty() && query.sort().is_empty());
        assert_eq!(query.order(), vec![(OrderKey::EntryId, Direction::Asc)]);
        assert_eq!(
            query.canonical_json(),
            r#"{"depth":0,"fields":null,"filters":[],"limit":20,"sort":[]}"#
        );
        // An explicit `[]` for filters or sort is the same query, canonically.
        let same = checked(&json!({"filters": [], "sort": [], "limit": 20, "depth": 0}))
            .expect("explicit defaults");
        assert_eq!(same.canonical_json(), query.canonical_json());
        assert_eq!(same, query);
    }

    #[test]
    fn the_document_is_checked_key_by_key() {
        let cases = [
            (json!([]), ("not_an_object", "")),
            (json!("select"), ("not_an_object", "")),
            (json!({"page": 2}), ("unknown_field", "/page")),
            (json!({"filters": {}}), ("wrong_type", "/filters")),
            (json!({"filters": ["x"]}), ("not_an_object", "/filters/0")),
            (
                json!({"filters": [{"field": "headline", "op": "eq", "value": "x", "not": 1}]}),
                ("unknown_field", "/filters/0/not"),
            ),
            (
                json!({"filters": [{"op": "eq", "value": "x"}]}),
                ("missing_field", "/filters/0/field"),
            ),
            (
                json!({"filters": [{"field": "headline", "value": "x"}]}),
                ("missing_field", "/filters/0/op"),
            ),
            (
                json!({"filters": [{"field": "headline", "op": "eq"}]}),
                ("missing_field", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": 3, "op": "eq", "value": "x"}]}),
                ("wrong_type", "/filters/0/field"),
            ),
            (
                json!({"filters": [{"field": "ghost", "op": "eq", "value": "x"}]}),
                ("unknown_field", "/filters/0/field"),
            ),
            (json!({"depth": 3}), ("out_of_range", "/depth")),
            (json!({"depth": -1}), ("out_of_range", "/depth")),
            (json!({"depth": 1.5}), ("out_of_range", "/depth")),
            (json!({"depth": "2"}), ("out_of_range", "/depth")),
            (json!({"depth": null}), ("out_of_range", "/depth")),
            (json!({"limit": 101}), ("out_of_range", "/limit")),
            (json!({"limit": 0}), ("out_of_range", "/limit")),
            (json!({"limit": 50.5}), ("out_of_range", "/limit")),
            (json!({"limit": "50"}), ("out_of_range", "/limit")),
        ];
        for (value, expected) in cases {
            let (code, pointer) = refusal_of(&value);
            assert_eq!((code, pointer.as_str()), expected, "{value}");
        }
        let nine: Vec<Value> = (0..9)
            .map(|_| json!({"field": "featured", "op": "eq", "value": true}))
            .collect();
        let (code, pointer) = refusal_of(&json!({"filters": nine}));
        assert_eq!(
            (code, pointer.as_str()),
            ("too_many", "/filters"),
            "nine filters is one too many"
        );
    }

    #[test]
    fn only_indexed_fields_filter_and_sort() {
        // Declared but not `index`ed: multiline text, rich text, a structure, a list,
        // a many reference, an asset, a colour, JSON, markdown — and plain text that
        // simply didn't opt in.
        for field in [
            "plain", "notes", "body", "meta", "tags", "related", "photo", "accent", "scratch",
            "script",
        ] {
            for (value, pointer) in [
                (
                    json!({"filters": [{"field": field, "op": "eq", "value": "x"}]}),
                    "/filters/0/field",
                ),
                (
                    json!({"sort": [{"field": field, "dir": "asc"}]}),
                    "/sort/0/field",
                ),
            ] {
                let (code, at) = refusal_of(&value);
                assert_eq!(
                    (code, at.as_str()),
                    ("field_not_indexed", pointer),
                    "{field}"
                );
            }
        }
        // `fields` doesn't filter or sort, so it may name any declared field — but only
        // declared ones.
        assert!(checked(&json!({"fields": ["body", "scratch"]})).is_ok());
        {
            let (code, pointer) = refusal_of(&json!({"fields": ["ghost"]}));
            assert_eq!((code, pointer.as_str()), ("unknown_field", "/fields/0"));
        }
        {
            let (code, pointer) = refusal_of(&json!({"fields": "headline"}));
            assert_eq!((code, pointer.as_str()), ("wrong_type", "/fields"));
        }
        {
            let (code, pointer) = refusal_of(&json!({"fields": []}));
            assert_eq!((code, pointer.as_str()), ("invalid_fields", "/fields"));
        }
        {
            let (code, pointer) = refusal_of(&json!({"fields": [3]}));
            assert_eq!((code, pointer.as_str()), ("wrong_type", "/fields/0"));
        }
        {
            let (code, pointer) = refusal_of(&json!({"fields": ["headline", "headline"]}));
            assert_eq!((code, pointer.as_str()), ("duplicate_field", "/fields/1"));
        }
        {
            let (code, pointer) = refusal_of(&json!({"fields": null}));
            assert_eq!((code, pointer.as_str()), ("wrong_type", "/fields"));
        }
    }

    #[test]
    fn operators_stay_within_what_a_kind_can_say() {
        let cases = [
            (
                json!({"filters": [{"field": "headline", "op": "like", "value": "x"}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "headline", "op": 3, "value": "x"}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "rating", "op": "contains", "value": "x"}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "featured", "op": "lt", "value": true}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "section", "op": "gt", "value": "News"}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "slug", "op": "contains", "value": "x"}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "email", "op": "gte", "value": "a@b.co"}]}),
                "invalid_op",
            ),
            (
                json!({"filters": [{"field": "author", "op": "lt", "value": ENTRY}]}),
                "invalid_op",
            ),
        ];
        for (value, expected) in cases {
            let (code, pointer) = refusal_of(&value);
            assert_eq!(
                (code, pointer.as_str()),
                (expected, "/filters/0/op"),
                "{value}"
            );
        }
        // Every kind that orders takes lt, lte, gt and gte.
        for field in [
            "headline",
            "rating",
            "score",
            "publish_at",
            "born",
            "opens",
            "slug",
        ] {
            for op in ["lt", "lte", "gt", "gte"] {
                let value = match field {
                    "rating" | "score" => json!(1),
                    "publish_at" => json!("2026-10-25T14:30:00Z"),
                    "born" => json!("2000-01-01"),
                    "opens" => json!("12:00"),
                    _ => json!("a"),
                };
                assert!(
                    checked(&json!({"filters": [{"field": field, "op": op, "value": value}]}))
                        .is_ok(),
                    "{field} {op}"
                );
            }
        }
    }

    #[test]
    fn values_are_checked_by_the_fields_own_rules() {
        let cases = [
            (
                json!({"filters": [{"field": "headline", "op": "eq", "value": 3}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "headline", "op": "eq", "value": " padded"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "headline", "op": "eq", "value": null}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "rating", "op": "eq", "value": "high"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "rating", "op": "eq", "value": 4.5}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "rating", "op": "gt", "value": 7}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "score", "op": "eq", "value": 1.000_000_1}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "featured", "op": "eq", "value": "yes"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "section", "op": "eq", "value": "Opinion"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "publish_at", "op": "lt", "value": "yesterday"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "born", "op": "eq", "value": "2026-02-29"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "opens", "op": "eq", "value": "25:00"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "email", "op": "eq", "value": "nope"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "site", "op": "eq", "value": "ftp://x"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "author", "op": "eq", "value": "not-a-uuid"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "slug", "op": "eq", "value": "Not A Slug"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
        ];
        for (value, expected) in cases {
            let (code, pointer) = refusal_of(&value);
            assert_eq!((code, pointer.as_str()), expected, "{value}");
        }
    }

    #[test]
    fn in_and_contains_have_their_own_value_rules() {
        let cases = [
            // `in` needs a list of 1 to 100 different values of the field's kind.
            (
                json!({"filters": [{"field": "section", "op": "in", "value": "News"}]}),
                ("wrong_type", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "section", "op": "in", "value": []}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "rating", "op": "in", "value": [1, "x"]}]}),
                ("invalid_value", "/filters/0/value/1"),
            ),
            (
                json!({"filters": [{"field": "section", "op": "in", "value": ["News", "News"]}]}),
                ("duplicate_value", "/filters/0/value"),
            ),
            // A `contains` needle is not a field value: it searches inside one.
            (
                json!({"filters": [{"field": "headline", "op": "contains", "value": 3}]}),
                ("wrong_type", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "headline", "op": "contains", "value": ""}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "headline", "op": "contains", "value": "bell\u{7}"}]}),
                ("invalid_value", "/filters/0/value"),
            ),
            (
                json!({"filters": [{"field": "headline", "op": "contains", "value": "x".repeat(121)}]}),
                ("invalid_value", "/filters/0/value"),
            ),
        ];
        for (value, expected) in cases {
            let (code, pointer) = refusal_of(&value);
            assert_eq!((code, pointer.as_str()), expected, "{value}");
        }
        let hundred_and_one: Vec<Value> = (0..=100).map(Value::from).collect();
        let (code, pointer) = refusal_of(
            &json!({"filters": [{"field": "score", "op": "in", "value": hundred_and_one}]}),
        );
        assert_eq!(
            (code, pointer.as_str()),
            ("invalid_value", "/filters/0/value"),
            "101 values is one too many"
        );
        // `in` on an integer field: two spellings of 2 are the same value.
        {
            let (code, pointer) = refusal_of(
                &json!({"filters": [{"field": "rating", "op": "in", "value": [2, 2.0]}]}),
            );
            assert_eq!(
                (code, pointer.as_str()),
                ("duplicate_value", "/filters/0/value")
            );
        }
    }

    #[test]
    fn the_sort_is_checked_and_the_tie_break_appended() {
        let cases = [
            (json!({"sort": "publish_at"}), ("wrong_type", "/sort")),
            (json!({"sort": [3]}), ("not_an_object", "/sort/0")),
            (
                json!({"sort": [{"field": "rating", "dir": "asc", "x": 1}]}),
                ("unknown_field", "/sort/0/x"),
            ),
            (
                json!({"sort": [{"dir": "desc"}]}),
                ("missing_field", "/sort/0/field"),
            ),
            (
                json!({"sort": [{"field": 4}]}),
                ("wrong_type", "/sort/0/field"),
            ),
            (
                json!({"sort": [{"field": "ghost"}]}),
                ("unknown_field", "/sort/0/field"),
            ),
            (
                json!({"sort": [{"field": "rating", "dir": "up"}]}),
                ("invalid_dir", "/sort/0/dir"),
            ),
            (
                json!({"sort": [{"field": "rating", "dir": 1}]}),
                ("invalid_dir", "/sort/0/dir"),
            ),
            (
                json!({"sort": [{"field": "rating"}, {"field": "rating", "dir": "desc"}]}),
                ("duplicate_field", "/sort/1/field"),
            ),
        ];
        for (value, expected) in cases {
            let (code, pointer) = refusal_of(&value);
            assert_eq!((code, pointer.as_str()), expected, "{value}");
        }
        let five = json!({"sort": [
            {"field": "rating"}, {"field": "score"}, {"field": "featured"},
            {"field": "born"}, {"field": "opens"}
        ]});
        let (code, pointer) = refusal_of(&five);
        assert_eq!(
            (code, pointer.as_str()),
            ("too_many", "/sort"),
            "five keys is one too many"
        );
        // The tie-break takes the last declared key's direction.
        let desc =
            checked(&json!({"sort": [{"field": "publish_at", "dir": "desc"}]})).expect("valid");
        assert_eq!(
            desc.order(),
            vec![
                (OrderKey::Field("publish_at".to_owned()), Direction::Desc),
                (OrderKey::EntryId, Direction::Desc),
            ]
        );
    }

    #[test]
    fn one_query_has_one_canonical_form_and_one_hash() {
        let first = checked(&json!({
            "sort": [{"field": "publish_at", "dir": "desc"}],
            "filters": [
                {"field": "section", "op": "in", "value": ["Guides", "News"]},
                {"op": "eq", "value": true, "field": "featured"}
            ],
            "depth": 1,
            "limit": 50,
            "fields": ["slug", "headline"]
        }))
        .expect("valid");
        // The same query with keys and lists in another order is the same bytes.
        let second = checked(&json!({
            "fields": ["headline", "slug"],
            "limit": 50,
            "depth": 1,
            "filters": [
                {"field": "featured", "op": "eq", "value": true},
                {"value": ["News", "Guides"], "field": "section", "op": "in"}
            ],
            "sort": [{"dir": "desc", "field": "publish_at"}]
        }))
        .expect("valid");
        assert_eq!(first, second);
        assert_eq!(
            first.canonical_json(),
            r#"{"depth":1,"fields":["headline","slug"],"filters":[{"field":"featured","op":"eq","value":true},{"field":"section","op":"in","value":["Guides","News"]}],"limit":50,"sort":[{"dir":"desc","field":"publish_at"}]}"#
        );
        assert_eq!(first.query_hash(), second.query_hash());
        // The hash pins the canonical form: SHA-256 of the string asserted above.
        assert_eq!(
            first.query_hash_hex(),
            "a7c8b173bbca25e7d826950259f306376959831c0e95cdb21dbd6cd850ca22a4"
        );
        assert_eq!(first.query_hash_hex().len(), 64);
        // A different query hashes differently.
        let other = checked(&json!({"limit": 51})).expect("valid");
        assert_ne!(first.query_hash(), other.query_hash());
    }

    #[test]
    fn index_is_a_declaration_the_definition_keeps() {
        // It parses, writes only when set, and round-trips: an old definition's bytes
        // and hash are untouched.
        let ty = post_type();
        let canonical = ty.canonical_json();
        assert!(
            canonical.contains(
                r#"{"id":"headline","label":"Headline","type":"text","required":true,"index":true,"max_chars":120,"multiline":false}"#
            ),
            "{canonical}"
        );
        assert!(
            !canonical
                .contains(r#""id":"plain","label":"Plain","type":"text","required":false,"index""#)
        );
        let again = ContentType::parse(&serde_json::from_str(&canonical).expect("JSON"))
            .expect("the canonical definition parses back");
        assert_eq!(again.canonical_json(), canonical);
        assert_eq!(
            ty.indexed_fields(),
            [
                "headline",
                "author",
                "publish_at",
                "section",
                "rating",
                "score",
                "featured",
                "slug",
                "email",
                "site",
                "born",
                "opens"
            ]
        );
        // Adding or dropping it changes no stored value, so it is a Compatible change.
        let mut dropped: Value = serde_json::from_str(&canonical).expect("JSON");
        for field in dropped["fields"].as_array_mut().expect("a list") {
            field.as_object_mut().expect("a field").remove("index");
        }
        let without = ContentType::parse(&dropped).expect("valid");
        assert_eq!(ty.successor(&without), crate::schema::Successor::Compatible);
        assert_eq!(without.successor(&ty), crate::schema::Successor::Compatible);
    }

    #[test]
    fn index_is_refused_where_it_cannot_apply() {
        let field = |extra: Value| {
            ContentType::parse(&json!({"name": "T", "title_field": "t", "fields": [
                {"id": "t", "label": "T", "type": "text", "required": true},
                extra
            ]}))
            .map(|_| ())
            .map_err(|refusal| (refusal.code, refusal.pointer))
        };
        let cases = [
            json!({"id": "x", "label": "X", "type": "rich_text", "index": true}),
            json!({"id": "x", "label": "X", "type": "text", "multiline": true, "index": true}),
            json!({"id": "x", "label": "X", "type": "json", "index": true}),
            json!({"id": "x", "label": "X", "type": "markdown", "index": true}),
            json!({"id": "x", "label": "X", "type": "color", "index": true}),
            json!({"id": "x", "label": "X", "type": "asset", "index": true}),
            json!({"id": "x", "label": "X", "type": "list", "of": {"type": "text"}, "index": true}),
            json!({"id": "x", "label": "X", "type": "blocks", "blocks": [
                {"name": "b", "label": "B", "fields": [{"id": "f", "label": "F", "type": "text"}]}
            ], "index": true}),
            json!({"id": "x", "label": "X", "type": "object", "fields": [
                {"id": "f", "label": "F", "type": "text"}
            ], "index": true}),
            json!({"id": "x", "label": "X", "type": "reference", "to": "articles", "many": true, "index": true}),
        ];
        for extra in cases {
            assert_eq!(
                field(extra.clone()),
                Err(("invalid_index", "/fields/1/index".to_owned())),
                "{extra}"
            );
        }
        // Nor inside a structure: only a content type's own fields are indexed.
        assert_eq!(
            field(
                json!({"id": "x", "label": "X", "type": "object", "fields": [
                    {"id": "f", "label": "F", "type": "text", "index": true}
                ]})
            ),
            Err(("invalid_index", "/fields/1/fields/0/index".to_owned()))
        );
        // It is a boolean.
        assert_eq!(
            field(json!({"id": "x", "label": "X", "type": "text", "index": "yes"})),
            Err(("wrong_type", "/fields/1/index".to_owned()))
        );
        // And every indexable kind takes it.
        for extra in [
            json!({"id": "x", "label": "X", "type": "text", "index": true}),
            json!({"id": "x", "label": "X", "type": "number", "index": true}),
            json!({"id": "x", "label": "X", "type": "boolean", "index": true}),
            json!({"id": "x", "label": "X", "type": "enum", "values": ["a"], "index": true}),
            json!({"id": "x", "label": "X", "type": "datetime", "index": true}),
            json!({"id": "x", "label": "X", "type": "date", "index": true}),
            json!({"id": "x", "label": "X", "type": "time", "index": true}),
            json!({"id": "x", "label": "X", "type": "email", "index": true}),
            json!({"id": "x", "label": "X", "type": "url", "index": true}),
            json!({"id": "x", "label": "X", "type": "slug", "index": true}),
            json!({"id": "x", "label": "X", "type": "reference", "to": "articles", "index": true}),
        ] {
            assert_eq!(field(extra.clone()), Ok(()), "{extra}");
        }
    }
}
