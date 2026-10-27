//! Content types: the versioned schemas that say what an entry's content may hold
//! (docs/modules/01-domain.md, "Schema model"; spec §07; D-040).
//!
//! A [`ContentType`] is parsed from its JSON definition, never trusted: every field has an id, a
//! label, a type and type-specific limits, and anything unrecognised is refused with a JSON
//! Pointer. It then validates content: unknown fields are refused (D11), required ones must be
//! there, each value must fit its field, and the result is written in one canonical form, the
//! exact bytes stored and hashed. The canonical form matches what `JSON.stringify` writes for the
//! same value, so the Studio can reproduce it.
//!
//! A definition can change only in ways that keep every stored entry valid
//! ([`ContentType::successor`]). New optional fields, wider limits, more enum options and new
//! labels are always safe. Removing a field, making one required or narrowing a limit is safe
//! only if no current draft or published revision is in the way, which only the stored content
//! can tell ([`ContentType::accepts`], D-055). Changing the title field, or turning a field into
//! or out of a slug, needs a migration, which doesn't exist yet.
//!
//! A slug field (D-054) holds an entry's URL name, unique in its collection: the database
//! enforces that, among drafts and among published entries. A content type has at most one.

use serde_json::{Map, Value};

use crate::rich_text::{self, json::write_string, pointer::Pointer};

/// Most fields in a content type.
pub const MAX_FIELDS: usize = 100;
/// Most characters a text field may allow.
pub const MAX_TEXT_CHARS: u64 = 100_000;
/// Most options an enum field may have.
pub const MAX_ENUM_OPTIONS: usize = 100;
/// The largest integer JavaScript represents exactly, so both ends agree on every value.
pub const MAX_SAFE_INTEGER: i64 = 9_007_199_254_740_991;
/// Decimal values stay below this magnitude, with at most [`DECIMAL_PLACES`] places, so they
/// have at most 15 significant digits and one canonical spelling in Rust and JavaScript alike.
pub const MAX_DECIMAL: f64 = 1e9;
/// Most decimal places a decimal number may have.
pub const DECIMAL_PLACES: i32 = 6;
/// Most characters in a slug.
pub const MAX_SLUG_CHARS: usize = 120;
/// The longest unknown key repeated back in an error.
const MAX_REPORTED_KEY_CHARS: usize = 64;

/// Why a definition or a value was refused: a stable code, a JSON Pointer, and a message for
/// people.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{message}")]
pub struct Refusal {
    /// The code clients branch on.
    pub code: &'static str,
    /// RFC 6901 JSON Pointer to the problem, from the document's root.
    pub pointer: String,
    /// What is wrong.
    pub message: String,
}

fn refuse(code: &'static str, pointer: &Pointer, message: impl Into<String>) -> Refusal {
    Refusal {
        code,
        pointer: pointer.as_str().to_owned(),
        message: message.into(),
    }
}

/// The kind of value a field holds, with its limits.
#[derive(Debug, Clone, PartialEq)]
pub enum FieldKind {
    /// A string of at most `max_chars` characters. Single-line text has no control characters
    /// and no spaces at either end; multi-line text may also hold line breaks and tabs.
    Text {
        /// The most characters allowed.
        max_chars: u64,
        /// Whether line breaks are allowed.
        multiline: bool,
    },
    /// A `colophon/rich-text` v1 document.
    RichText,
    /// A number, optionally a whole number, optionally bounded.
    Number {
        /// Only whole numbers (within JavaScript's safe range).
        integer: bool,
        /// The smallest value allowed.
        min: Option<f64>,
        /// The largest value allowed.
        max: Option<f64>,
    },
    /// `true` or `false`.
    Boolean,
    /// One of a fixed list of strings.
    Enum {
        /// The allowed values, in display order.
        values: Vec<String>,
    },
    /// A moment in UTC, written `YYYY-MM-DDTHH:MM:SSZ`.
    DateTime,
    /// The entry's URL name ([`is_slug`]), unique in its collection.
    Slug {
        /// The single-line text field the Studio suggests the slug from. Only a hint: the
        /// server never derives a slug.
        source: Option<String>,
    },
}

impl FieldKind {
    /// The name in definitions.
    pub const fn name(&self) -> &'static str {
        match self {
            Self::Text { .. } => "text",
            Self::RichText => "rich_text",
            Self::Number { .. } => "number",
            Self::Boolean => "boolean",
            Self::Enum { .. } => "enum",
            Self::DateTime => "datetime",
            Self::Slug { .. } => "slug",
        }
    }
}

/// What it takes to replace a definition with a new one ([`ContentType::successor`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Successor {
    /// The new definition accepts everything the old one does: nothing stored needs checking.
    Compatible,
    /// The new definition is narrower. It may replace the old one only if it
    /// [accepts](ContentType::accepts) every current draft and published revision; this says
    /// why the check is needed.
    CheckContent(Refusal),
    /// Not without a migration, whatever is stored.
    Refused(Refusal),
}

/// One field of a content type.
#[derive(Debug, Clone, PartialEq)]
pub struct Field {
    /// Its id: `^[a-z][a-z0-9_]{0,63}$`, the key in content.
    pub id: String,
    /// What editors see.
    pub label: String,
    /// Whether content must have it.
    pub required: bool,
    /// What it holds.
    pub kind: FieldKind,
}

/// A parsed, valid content type definition.
#[derive(Debug, Clone, PartialEq)]
pub struct ContentType {
    name: String,
    title_field: String,
    fields: Vec<Field>,
}

/// Content that passed validation, in canonical form.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Content {
    canonical: String,
    title: String,
    slug: Option<String>,
}

impl Content {
    /// The canonical JSON: the exact text stored and hashed.
    pub fn canonical_json(&self) -> &str {
        &self.canonical
    }

    /// The value of the content type's title field, for lists.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// The value of the content type's slug field, if it has one and the content sets it.
    pub fn slug(&self) -> Option<&str> {
        self.slug.as_deref()
    }
}

/// Whether `text` is a slug: 1 to [`MAX_SLUG_CHARS`] lowercase ASCII letters and digits, in
/// groups joined by single hyphens, like `my-first-post`. Refused rather than normalized, like
/// all text: the Studio suggests a slug, and what is stored is what was sent.
pub fn is_slug(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=MAX_SLUG_CHARS).contains(&bytes.len())
        && bytes.split(|byte| *byte == b'-').all(|group| {
            !group.is_empty()
                && group
                    .iter()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit())
        })
}

fn plain_text(text: &str, max: usize) -> bool {
    let length = text.chars().count();
    (1..=max).contains(&length) && !text.chars().any(char::is_control) && text.trim() == text
}

fn is_field_id(text: &str) -> bool {
    let bytes = text.as_bytes();
    (1..=64).contains(&bytes.len())
        && bytes.first().is_some_and(u8::is_ascii_lowercase)
        && bytes
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'_')
}

/// Refuses any key of `object` outside `allowed`. Never dropped silently (D11).
fn only(object: &Map<String, Value>, allowed: &[&str], pointer: &Pointer) -> Result<(), Refusal> {
    match object.keys().find(|key| !allowed.contains(&key.as_str())) {
        Some(key) => {
            let shown: String = key.chars().take(MAX_REPORTED_KEY_CHARS).collect();
            Err(refuse(
                "unknown_field",
                &at(pointer, &shown),
                format!("`{shown}` is not allowed here"),
            ))
        }
        None => Ok(()),
    }
}

fn bool_or(
    object: &Map<String, Value>,
    key: &str,
    default: bool,
    pointer: &Pointer,
) -> Result<bool, Refusal> {
    match object.get(key) {
        None => Ok(default),
        Some(Value::Bool(value)) => Ok(*value),
        Some(_) => Err(refuse(
            "wrong_type",
            &at(pointer, key),
            format!("`{key}` is true or false"),
        )),
    }
}

/// `pointer` with one more segment.
fn at(pointer: &Pointer, key: &str) -> Pointer {
    let mut next = pointer.clone();
    next.push(key);
    next
}

/// A finite number in definitions, within the limits content may use.
fn bound(
    value: Option<&Value>,
    key: &str,
    integer: bool,
    pointer: &Pointer,
) -> Result<Option<f64>, Refusal> {
    let Some(value) = value else { return Ok(None) };
    let here = at(pointer, key);
    let number = value
        .as_f64()
        .ok_or_else(|| refuse("wrong_type", &here, format!("`{key}` is a number")))?;
    check_number(number, integer).map_err(|message| refuse("out_of_range", &here, message))?;
    Ok(Some(number))
}

/// Whether `number` is a value content may hold: a safe integer, or a decimal with at most six
/// places below 10^9.
#[expect(clippy::float_cmp, reason = "exactness is what is being checked")]
fn check_number(number: f64, integer: bool) -> Result<(), String> {
    if !number.is_finite() {
        return Err("numbers must be finite".to_owned());
    }
    if integer {
        // The cast is exact: the bound below keeps the value inside i64.
        #[expect(
            clippy::cast_precision_loss,
            reason = "MAX_SAFE_INTEGER is exactly representable"
        )]
        let safe = MAX_SAFE_INTEGER as f64;
        if number.fract() != 0.0 || number.abs() > safe {
            return Err(format!(
                "a whole number between -{MAX_SAFE_INTEGER} and {MAX_SAFE_INTEGER}"
            ));
        }
        return Ok(());
    }
    if number.abs() >= MAX_DECIMAL {
        return Err("decimals stay below 1,000,000,000 in size".to_owned());
    }
    let scale = 10_f64.powi(DECIMAL_PLACES);
    if (number * scale).round() / scale != number {
        return Err(format!("at most {DECIMAL_PLACES} decimal places"));
    }
    Ok(())
}

/// A number as `JSON.stringify` writes it, for values [`check_number`] accepts.
fn write_number(out: &mut String, number: f64, integer: bool) {
    // -0 is written as 0 by JavaScript.
    let number = if number == 0.0 { 0.0 } else { number };
    if integer {
        #[expect(clippy::cast_possible_truncation, reason = "a checked safe integer")]
        let whole = number as i64;
        out.push_str(&whole.to_string());
        return;
    }
    // At most 15 significant digits: the trimmed six-place form is the shortest spelling that
    // round-trips, which is what JavaScript prints in this range.
    let fixed = format!("{number:.6}");
    let trimmed = fixed.trim_end_matches('0').trim_end_matches('.');
    out.push_str(trimmed);
}

/// `YYYY-MM-DDTHH:MM:SSZ`, a real calendar date and time.
fn is_utc_timestamp(text: &str) -> bool {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 20
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            10 => *byte == b'T',
            13 | 16 => *byte == b':',
            19 => *byte == b'Z',
            _ => byte.is_ascii_digit(),
        });
    if !shape {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> u32 {
        text.get(range)
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(u32::MAX)
    };
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let (hour, minute, second) = (number(11..13), number(14..16), number(17..19));
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    let days = match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    };
    year >= 1 && (1..=days).contains(&day) && hour < 24 && minute < 60 && second < 60
}

/// A text field's limits: `max_chars` (default 200) and `multiline` (default false).
fn text_kind(object: &Map<String, Value>, pointer: &Pointer) -> Result<FieldKind, Refusal> {
    let max_chars = match object.get("max_chars") {
        None => 200,
        Some(value) => value
            .as_u64()
            .filter(|max| (1..=MAX_TEXT_CHARS).contains(max))
            .ok_or_else(|| {
                refuse(
                    "out_of_range",
                    &at(pointer, "max_chars"),
                    format!("`max_chars` is a whole number from 1 to {MAX_TEXT_CHARS}"),
                )
            })?,
    };
    Ok(FieldKind::Text {
        max_chars,
        multiline: bool_or(object, "multiline", false, pointer)?,
    })
}

/// A number field's limits: `integer` (default false), and optional `min` and `max`.
fn number_kind(object: &Map<String, Value>, pointer: &Pointer) -> Result<FieldKind, Refusal> {
    let integer = bool_or(object, "integer", false, pointer)?;
    let min = bound(object.get("min"), "min", integer, pointer)?;
    let max = bound(object.get("max"), "max", integer, pointer)?;
    if let (Some(min), Some(max)) = (min, max)
        && min > max
    {
        return Err(refuse(
            "out_of_range",
            &at(pointer, "max"),
            "`max` is below `min`",
        ));
    }
    Ok(FieldKind::Number { integer, min, max })
}

/// A text value: within `max_chars`, not empty (an absent optional field is left out), and
/// without control characters, except line breaks and tabs in multi-line text. Single-line text
/// also has no spaces at either end: refused rather than trimmed, so what is stored is what was
/// sent.
fn check_text(
    text: &str,
    label: &str,
    max_chars: u64,
    multiline: bool,
    pointer: &Pointer,
) -> Result<(), Refusal> {
    let length = u64::try_from(text.chars().count()).unwrap_or(u64::MAX);
    if length > max_chars {
        return Err(refuse(
            "too_long",
            pointer,
            format!("{label} is at most {max_chars} characters"),
        ));
    }
    if text.is_empty() {
        return Err(refuse(
            "invalid_text",
            pointer,
            format!("{label} is empty; leave it out instead"),
        ));
    }
    let clean = if multiline {
        !text
            .chars()
            .any(|c| c.is_control() && c != '\n' && c != '\t')
    } else {
        !text.chars().any(char::is_control) && text.trim() == text
    };
    if clean {
        Ok(())
    } else if multiline {
        Err(refuse(
            "invalid_text",
            pointer,
            format!("{label} may hold line breaks and tabs, but no other control characters"),
        ))
    } else {
        Err(refuse(
            "invalid_text",
            pointer,
            format!("{label} has no control characters or spaces at either end"),
        ))
    }
}

/// Whether every value `old` accepts is also accepted by `new`.
fn widens(old: &FieldKind, new: &FieldKind) -> bool {
    match (old, new) {
        (
            FieldKind::Text {
                max_chars: old_max,
                multiline: old_multiline,
            },
            FieldKind::Text {
                max_chars: new_max,
                multiline: new_multiline,
            },
        ) => new_max >= old_max && (*new_multiline || !old_multiline),
        (FieldKind::RichText, FieldKind::RichText)
        | (FieldKind::Boolean, FieldKind::Boolean)
        | (FieldKind::DateTime, FieldKind::DateTime)
        | (FieldKind::Slug { .. }, FieldKind::Slug { .. }) => true,
        (
            FieldKind::Number {
                integer: old_integer,
                min: old_min,
                max: old_max,
            },
            FieldKind::Number {
                integer: new_integer,
                min: new_min,
                max: new_max,
            },
        ) => {
            // A decimal field accepts every integer; a new bound must not cut off old values.
            (*old_integer || !new_integer)
                && new_min.is_none_or(|new_min| old_min.is_some_and(|old_min| new_min <= old_min))
                && new_max.is_none_or(|new_max| old_max.is_some_and(|old_max| new_max >= old_max))
        }
        (FieldKind::Enum { values: old_values }, FieldKind::Enum { values: new_values }) => {
            old_values.iter().all(|value| new_values.contains(value))
        }
        _ => false,
    }
}

/// An enum field's options: distinct, plain, 1 to [`MAX_ENUM_OPTIONS`].
fn enum_values(object: &Map<String, Value>, pointer: &Pointer) -> Result<Vec<String>, Refusal> {
    let list = object
        .get("values")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            refuse(
                "missing_field",
                &at(pointer, "values"),
                "`values` lists the options",
            )
        })?;
    if list.is_empty() || list.len() > MAX_ENUM_OPTIONS {
        return Err(refuse(
            "invalid_values",
            &at(pointer, "values"),
            format!("an enum has 1 to {MAX_ENUM_OPTIONS} options"),
        ));
    }
    let mut values: Vec<String> = Vec::with_capacity(list.len());
    for (index, option) in list.iter().enumerate() {
        let mut here = at(pointer, "values");
        here.push_index(index);
        let text = option
            .as_str()
            .filter(|text| plain_text(text, 100) && !values.iter().any(|seen| seen == text))
            .ok_or_else(|| {
                refuse(
                    "invalid_values",
                    &here,
                    "options are distinct, 1 to 100 characters, without control characters or spaces at either end",
                )
            })?;
        values.push(text.to_owned());
    }
    Ok(values)
}

impl ContentType {
    /// The built-in content type every project starts with: a title and a rich-text body. Its
    /// canonical content is byte for byte what articles stored before content types existed.
    pub fn articles() -> Self {
        Self {
            name: "Articles".to_owned(),
            title_field: "title".to_owned(),
            fields: vec![
                Field {
                    id: "title".to_owned(),
                    label: "Title".to_owned(),
                    required: true,
                    kind: FieldKind::Text {
                        max_chars: 200,
                        multiline: false,
                    },
                },
                Field {
                    id: "body".to_owned(),
                    label: "Body".to_owned(),
                    required: true,
                    kind: FieldKind::RichText,
                },
            ],
        }
    }

    /// Parses a definition: `{"name", "title_field", "fields": [...]}`.
    pub fn parse(value: &Value) -> Result<Self, Refusal> {
        let root = Pointer::new();
        let object = value
            .as_object()
            .ok_or_else(|| refuse("not_an_object", &root, "a definition is a JSON object"))?;
        only(object, &["name", "title_field", "fields"], &root)?;
        let name = object
            .get("name")
            .and_then(Value::as_str)
            .filter(|name| plain_text(name, 100))
            .ok_or_else(|| {
                refuse(
                    "invalid_name",
                    &at(&root, "name"),
                    "a name is 1 to 100 characters, without control characters or spaces at either end",
                )
            })?
            .to_owned();
        let list = object
            .get("fields")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                refuse(
                    "missing_field",
                    &at(&root, "fields"),
                    "`fields` is a list of fields",
                )
            })?;
        if list.is_empty() || list.len() > MAX_FIELDS {
            return Err(refuse(
                "invalid_fields",
                &at(&root, "fields"),
                format!("a content type has 1 to {MAX_FIELDS} fields"),
            ));
        }
        let mut fields: Vec<Field> = Vec::with_capacity(list.len());
        for (index, item) in list.iter().enumerate() {
            let mut pointer = at(&root, "fields");
            pointer.push_index(index);
            let field = Self::field(item, &pointer)?;
            if fields.iter().any(|seen| seen.id == field.id) {
                return Err(refuse(
                    "duplicate_field",
                    &at(&pointer, "id"),
                    format!("`{}` is used by two fields", field.id),
                ));
            }
            let is_slug = |field: &Field| matches!(field.kind, FieldKind::Slug { .. });
            if is_slug(&field) && fields.iter().any(is_slug) {
                return Err(refuse(
                    "duplicate_slug",
                    &at(&pointer, "type"),
                    "a content type has at most one slug field",
                ));
            }
            fields.push(field);
        }
        for (index, field) in fields.iter().enumerate() {
            let FieldKind::Slug {
                source: Some(source),
            } = &field.kind
            else {
                continue;
            };
            let single_line = fields.iter().any(|other| {
                other.id == *source
                    && matches!(
                        other.kind,
                        FieldKind::Text {
                            multiline: false,
                            ..
                        }
                    )
            });
            if !single_line {
                let mut pointer = at(&root, "fields");
                pointer.push_index(index);
                return Err(refuse(
                    "invalid_source",
                    &at(&pointer, "source"),
                    "`source` must name a single-line text field of this content type",
                ));
            }
        }
        let title_field = object
            .get("title_field")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                refuse(
                    "missing_field",
                    &at(&root, "title_field"),
                    "`title_field` names the field used as the title",
                )
            })?
            .to_owned();
        let usable = fields.iter().any(|field| {
            field.id == title_field
                && field.required
                && matches!(
                    field.kind,
                    FieldKind::Text {
                        multiline: false,
                        ..
                    }
                )
        });
        if !usable {
            return Err(refuse(
                "invalid_title_field",
                &at(&root, "title_field"),
                "`title_field` must name a required, single-line text field",
            ));
        }
        Ok(Self {
            name,
            title_field,
            fields,
        })
    }

    fn field(value: &Value, pointer: &Pointer) -> Result<Field, Refusal> {
        let object = value
            .as_object()
            .ok_or_else(|| refuse("not_an_object", pointer, "a field is a JSON object"))?;
        let id = object
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| is_field_id(id))
            .ok_or_else(|| {
                refuse(
                    "invalid_id",
                    &at(pointer, "id"),
                    "a field id is 1 to 64 lowercase letters, digits and underscores, starting with a letter",
                )
            })?
            .to_owned();
        let label = object
            .get("label")
            .and_then(Value::as_str)
            .filter(|label| plain_text(label, 100))
            .ok_or_else(|| {
                refuse(
                    "invalid_label",
                    &at(pointer, "label"),
                    "a label is 1 to 100 characters, without control characters or spaces at either end",
                )
            })?
            .to_owned();
        let required = bool_or(object, "required", false, pointer)?;
        let common = ["id", "label", "type", "required"];
        let kind = match object.get("type").and_then(Value::as_str) {
            Some("text") => {
                only(
                    object,
                    &[&common[..], &["max_chars", "multiline"]].concat(),
                    pointer,
                )?;
                text_kind(object, pointer)?
            }
            Some("rich_text") => {
                only(object, &common, pointer)?;
                FieldKind::RichText
            }
            Some("number") => {
                only(
                    object,
                    &[&common[..], &["integer", "min", "max"]].concat(),
                    pointer,
                )?;
                number_kind(object, pointer)?
            }
            Some("boolean") => {
                only(object, &common, pointer)?;
                FieldKind::Boolean
            }
            Some("enum") => {
                only(object, &[&common[..], &["values"]].concat(), pointer)?;
                FieldKind::Enum {
                    values: enum_values(object, pointer)?,
                }
            }
            Some("datetime") => {
                only(object, &common, pointer)?;
                FieldKind::DateTime
            }
            Some("slug") => {
                only(object, &[&common[..], &["source"]].concat(), pointer)?;
                let source = match object.get("source") {
                    None => None,
                    Some(value) => Some(
                        value
                            .as_str()
                            .filter(|source| is_field_id(source))
                            .ok_or_else(|| {
                                refuse(
                                    "invalid_source",
                                    &at(pointer, "source"),
                                    "`source` is the id of a field",
                                )
                            })?
                            .to_owned(),
                    ),
                };
                FieldKind::Slug { source }
            }
            _ => {
                return Err(refuse(
                    "invalid_type",
                    &at(pointer, "type"),
                    "`type` is one of text, rich_text, number, boolean, enum, datetime, slug",
                ));
            }
        };
        Ok(Field {
            id,
            label,
            required,
            kind,
        })
    }

    /// The name.
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The id of the field used as the title.
    pub fn title_field(&self) -> &str {
        &self.title_field
    }

    /// The fields, in order.
    pub fn fields(&self) -> &[Field] {
        &self.fields
    }

    /// The definition in canonical form, every default written out: the bytes stored and hashed.
    pub fn canonical_json(&self) -> String {
        let mut out = String::with_capacity(256);
        out.push_str(r#"{"name":"#);
        write_string(&mut out, &self.name);
        out.push_str(r#","title_field":"#);
        write_string(&mut out, &self.title_field);
        out.push_str(r#","fields":["#);
        for (index, field) in self.fields.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            out.push_str(r#"{"id":"#);
            write_string(&mut out, &field.id);
            out.push_str(r#","label":"#);
            write_string(&mut out, &field.label);
            out.push_str(r#","type":"#);
            write_string(&mut out, field.kind.name());
            out.push_str(if field.required {
                r#","required":true"#
            } else {
                r#","required":false"#
            });
            match &field.kind {
                FieldKind::Text {
                    max_chars,
                    multiline,
                } => {
                    out.push_str(r#","max_chars":"#);
                    out.push_str(&max_chars.to_string());
                    out.push_str(if *multiline {
                        r#","multiline":true"#
                    } else {
                        r#","multiline":false"#
                    });
                }
                FieldKind::Number { integer, min, max } => {
                    out.push_str(if *integer {
                        r#","integer":true"#
                    } else {
                        r#","integer":false"#
                    });
                    for (key, value) in [(r#","min":"#, min), (r#","max":"#, max)] {
                        if let Some(value) = value {
                            out.push_str(key);
                            write_number(&mut out, *value, *integer);
                        }
                    }
                }
                FieldKind::Enum { values } => {
                    out.push_str(r#","values":["#);
                    for (index, value) in values.iter().enumerate() {
                        if index > 0 {
                            out.push(',');
                        }
                        write_string(&mut out, value);
                    }
                    out.push(']');
                }
                FieldKind::Slug { source } => {
                    if let Some(source) = source {
                        out.push_str(r#","source":"#);
                        write_string(&mut out, source);
                    }
                }
                FieldKind::RichText | FieldKind::Boolean | FieldKind::DateTime => {}
            }
            out.push('}');
        }
        out.push_str("]}");
        out
    }

    /// Validates content against this definition and writes it in canonical form: fields in
    /// definition order, absent optional fields left out.
    pub fn validate(&self, value: &Value) -> Result<Content, Refusal> {
        let root = Pointer::new();
        let object = value
            .as_object()
            .ok_or_else(|| refuse("not_an_object", &root, "content is a JSON object"))?;
        let ids: Vec<&str> = self.fields.iter().map(|field| field.id.as_str()).collect();
        only(object, &ids, &root)?;
        let mut out = String::with_capacity(256);
        let mut title = String::new();
        let mut slug = None;
        out.push('{');
        let mut first = true;
        for field in &self.fields {
            let pointer = at(&root, &field.id);
            let Some(value) = object.get(&field.id) else {
                if field.required {
                    return Err(refuse(
                        "missing_field",
                        &pointer,
                        format!("`{}` is required", field.label),
                    ));
                }
                continue;
            };
            if !first {
                out.push(',');
            }
            first = false;
            write_string(&mut out, &field.id);
            out.push(':');
            Self::value(field, value, &pointer, &mut out)?;
            if field.id == self.title_field {
                value.as_str().unwrap_or_default().clone_into(&mut title);
            }
            if matches!(field.kind, FieldKind::Slug { .. }) {
                slug = value.as_str().map(str::to_owned);
            }
        }
        out.push('}');
        Ok(Content {
            canonical: out,
            title,
            slug,
        })
    }

    /// Whether this definition accepts `stored`, the canonical content of a revision validated
    /// against an earlier version: the check behind [`Successor::CheckContent`]. Stored content
    /// is never rewritten; it only has to stay valid.
    pub fn accepts(&self, stored: &str) -> Result<(), Refusal> {
        let value: Value = serde_json::from_str(stored).map_err(|_| {
            refuse(
                "unreadable",
                &Pointer::new(),
                "the stored content is not JSON",
            )
        })?;
        self.validate(&value).map(|_| ())
    }

    fn value(
        field: &Field,
        value: &Value,
        pointer: &Pointer,
        out: &mut String,
    ) -> Result<(), Refusal> {
        let label = &field.label;
        match &field.kind {
            FieldKind::Text {
                max_chars,
                multiline,
            } => {
                let text = value
                    .as_str()
                    .ok_or_else(|| refuse("wrong_type", pointer, format!("{label} is text")))?;
                check_text(text, label, *max_chars, *multiline, pointer)?;
                write_string(out, text);
            }
            FieldKind::RichText => {
                let document = rich_text::validate(value).map_err(|rejection| Refusal {
                    code: rejection.code.as_str(),
                    pointer: format!("{}{}", pointer.as_str(), rejection.pointer),
                    message: rejection.to_string(),
                })?;
                out.push_str(document.canonical_json());
            }
            FieldKind::Number { integer, min, max } => {
                let number = value
                    .as_f64()
                    .ok_or_else(|| refuse("wrong_type", pointer, format!("{label} is a number")))?;
                check_number(number, *integer).map_err(|message| {
                    refuse(
                        if *integer {
                            "not_an_integer"
                        } else {
                            "too_precise"
                        },
                        pointer,
                        format!("{label}: {message}"),
                    )
                })?;
                if min.is_some_and(|min| number < min) || max.is_some_and(|max| number > max) {
                    return Err(refuse(
                        "out_of_range",
                        pointer,
                        format!("{label} is outside its allowed range"),
                    ));
                }
                write_number(out, number, *integer);
            }
            FieldKind::Boolean => {
                let flag = value.as_bool().ok_or_else(|| {
                    refuse("wrong_type", pointer, format!("{label} is true or false"))
                })?;
                out.push_str(if flag { "true" } else { "false" });
            }
            FieldKind::Enum { values } => {
                let text = value
                    .as_str()
                    .filter(|text| values.iter().any(|option| option == text))
                    .ok_or_else(|| {
                        refuse(
                            "not_an_option",
                            pointer,
                            format!("{label} is one of its options"),
                        )
                    })?;
                write_string(out, text);
            }
            FieldKind::DateTime => {
                let text = value
                    .as_str()
                    .filter(|text| is_utc_timestamp(text))
                    .ok_or_else(|| {
                        refuse(
                            "invalid_datetime",
                            pointer,
                            format!("{label} is a UTC time like 2026-10-25T14:30:00Z"),
                        )
                    })?;
                write_string(out, text);
            }
            FieldKind::Slug { .. } => {
                let text = value
                    .as_str()
                    .filter(|text| is_slug(text))
                    .ok_or_else(|| {
                        refuse(
                            "invalid_slug",
                            pointer,
                            format!(
                                "{label} is 1 to {MAX_SLUG_CHARS} lowercase letters and digits, in groups joined by single hyphens, like my-first-post"
                            ),
                        )
                    })?;
                write_string(out, text);
            }
        }
        Ok(())
    }

    /// What it takes for `next` to replace this definition.
    ///
    /// [`Successor::Compatible`]: a new name or labels, new optional fields, wider limits, more
    /// enum options, an integer field becoming a decimal one, a single-line text becoming
    /// multi-line.
    ///
    /// [`Successor::CheckContent`]: anything else that only stored content can rule out, such as
    /// removing a field, making one required, narrowing a limit or changing a field's type. The
    /// refusal is the first such change, phrased for when stored content is in the way.
    ///
    /// [`Successor::Refused`]: a different title field (every revision's title is stored for
    /// lists), or a field turned into or out of a slug (every draft's and publication's slug is
    /// stored for uniqueness). Both need a migration.
    pub fn successor(&self, next: &Self) -> Successor {
        let root = Pointer::new();
        if next.title_field != self.title_field {
            return Successor::Refused(refuse(
                "needs_migration",
                &at(&root, "title_field"),
                "changing the title field needs a migration",
            ));
        }
        let slug = |field: &Field| matches!(field.kind, FieldKind::Slug { .. });
        for (index, new) in next.fields.iter().enumerate() {
            if let Some(old) = self.fields.iter().find(|old| old.id == new.id)
                && slug(old) != slug(new)
            {
                let mut pointer = at(&root, "fields");
                pointer.push_index(index);
                return Successor::Refused(refuse(
                    "needs_migration",
                    &pointer,
                    format!(
                        "turning `{}` into or out of a slug field needs a migration",
                        new.id
                    ),
                ));
            }
        }
        for old in &self.fields {
            let Some((index, new)) = next
                .fields
                .iter()
                .enumerate()
                .find(|(_, field)| field.id == old.id)
            else {
                return Successor::CheckContent(refuse(
                    "needs_migration",
                    &at(&root, "fields"),
                    format!("removing `{}` would make stored content invalid", old.id),
                ));
            };
            let mut pointer = at(&root, "fields");
            pointer.push_index(index);
            if !widens(&old.kind, &new.kind) || (new.required && !old.required) {
                return Successor::CheckContent(refuse(
                    "needs_migration",
                    &pointer,
                    format!(
                        "this change to `{}` would make stored content invalid",
                        old.id
                    ),
                ));
            }
        }
        for (index, new) in next.fields.iter().enumerate() {
            if new.required && !self.fields.iter().any(|old| old.id == new.id) {
                let mut pointer = at(&root, "fields");
                pointer.push_index(index);
                return Successor::CheckContent(refuse(
                    "needs_migration",
                    &pointer,
                    format!(
                        "a new required field (`{}`) would make stored content invalid",
                        new.id
                    ),
                ));
            }
        }
        Successor::Compatible
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn post_type() -> ContentType {
        ContentType::parse(&json!({
            "name": "Posts",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120},
                {"id": "summary", "label": "Summary", "type": "text", "multiline": true, "max_chars": 500},
                {"id": "body", "label": "Body", "type": "rich_text", "required": true},
                {"id": "reading_minutes", "label": "Reading time", "type": "number", "integer": true, "min": 1, "max": 120},
                {"id": "rating", "label": "Rating", "type": "number", "min": 0, "max": 5},
                {"id": "featured", "label": "Featured", "type": "boolean"},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"]},
                {"id": "publish_at", "label": "Publish at", "type": "datetime"}
            ]
        }))
        .expect("a valid definition")
    }

    fn body() -> Value {
        json!({"schema": "colophon/rich-text", "version": 1, "doc": {"type": "doc", "content": [
            {"type": "paragraph", "content": [{"type": "text", "text": "Hello"}]}
        ]}})
    }

    #[test]
    fn articles_store_exactly_what_they_did_before_content_types() {
        let content = ContentType::articles()
            .validate(&json!({"title": "A \"quoted\" title, é", "body": body()}))
            .expect("valid");
        let rich = rich_text::validate(&body()).expect("valid body");
        assert_eq!(
            content.canonical_json(),
            format!(
                r#"{{"title":"A \"quoted\" title, é","body":{}}}"#,
                rich.canonical_json()
            )
        );
        assert_eq!(content.title(), "A \"quoted\" title, é");
    }

    #[test]
    fn the_canonical_definition_writes_every_default_and_parses_back_to_itself() {
        let canonical = post_type().canonical_json();
        assert!(canonical.starts_with(r#"{"name":"Posts","title_field":"headline","fields":[{"id":"headline","label":"Headline","type":"text","required":true,"max_chars":120,"multiline":false}"#), "{canonical}");
        assert!(canonical.contains(r#"{"id":"rating","label":"Rating","type":"number","required":false,"integer":false,"min":0,"max":5}"#), "{canonical}");
        let again =
            ContentType::parse(&serde_json::from_str(&canonical).expect("JSON")).expect("valid");
        assert_eq!(again.canonical_json(), canonical);
        assert_eq!(
            ContentType::articles().canonical_json(),
            r#"{"name":"Articles","title_field":"title","fields":[{"id":"title","label":"Title","type":"text","required":true,"max_chars":200,"multiline":false},{"id":"body","label":"Body","type":"rich_text","required":true}]}"#
        );
    }

    #[test]
    fn content_is_written_in_definition_order_with_canonical_numbers() {
        let content = post_type()
            .validate(&json!({
                "publish_at": "2028-02-29T23:59:59Z",
                "rating": 4.5,
                "body": body(),
                "headline": "Hi",
                "reading_minutes": 7,
                "featured": false,
                "section": "Guides",
                "summary": "Line one\nline two"
            }))
            .expect("valid");
        let rich = rich_text::validate(&body()).expect("valid body");
        assert_eq!(
            content.canonical_json(),
            format!(
                r#"{{"headline":"Hi","summary":"Line one\nline two","body":{},"reading_minutes":7,"rating":4.5,"featured":false,"section":"Guides","publish_at":"2028-02-29T23:59:59Z"}}"#,
                rich.canonical_json()
            )
        );
        assert_eq!(content.title(), "Hi");
    }

    #[test]
    fn numbers_are_written_the_way_javascript_writes_them() {
        let cases = [
            (0.1, "0.1"),
            (0.000_001, "0.000001"),
            (2.675, "2.675"),
            (-0.0, "0"),
            (999_999_999.999_999, "999999999.999999"),
            (100.0, "100"),
            (-12.5, "-12.5"),
        ];
        for (number, expected) in cases {
            let mut out = String::new();
            write_number(&mut out, number, false);
            assert_eq!(out, expected, "{number}");
        }
        let mut out = String::new();
        write_number(&mut out, 9_007_199_254_740_991.0, true);
        assert_eq!(out, "9007199254740991");
    }

    #[test]
    fn content_that_does_not_fit_is_refused_with_a_pointer() {
        let ty = post_type();
        let valid = || json!({"headline": "Hi", "body": body()});
        let with = |key: &str, value: Value| {
            let mut content = valid();
            content[key] = value;
            content
        };
        let cases = [
            (json!([]), "not_an_object", ""),
            (json!({"body": body()}), "missing_field", "/headline"),
            (with("extra", json!(1)), "unknown_field", "/extra"),
            (
                with("headline", json!(" padded")),
                "invalid_text",
                "/headline",
            ),
            (
                with("headline", json!("x".repeat(121))),
                "too_long",
                "/headline",
            ),
            (
                with("summary", json!("bell\u{7}")),
                "invalid_text",
                "/summary",
            ),
            (with("summary", json!("")), "invalid_text", "/summary"),
            (
                with("reading_minutes", json!(1.5)),
                "not_an_integer",
                "/reading_minutes",
            ),
            (
                with("reading_minutes", json!(121)),
                "out_of_range",
                "/reading_minutes",
            ),
            (with("rating", json!(4.123_456_7)), "too_precise", "/rating"),
            (with("rating", json!(-1)), "out_of_range", "/rating"),
            (with("featured", json!("yes")), "wrong_type", "/featured"),
            (
                with("section", json!("Opinion")),
                "not_an_option",
                "/section",
            ),
            (
                with("publish_at", json!("2026-02-29T10:00:00Z")),
                "invalid_datetime",
                "/publish_at",
            ),
            (
                with("publish_at", json!("2026-10-25T10:00:00+02:00")),
                "invalid_datetime",
                "/publish_at",
            ),
            (
                with("publish_at", json!(null)),
                "invalid_datetime",
                "/publish_at",
            ),
        ];
        for (content, code, pointer) in cases {
            let refusal = ty.validate(&content).expect_err("refused");
            assert_eq!(
                (refusal.code, refusal.pointer.as_str()),
                (code, pointer),
                "{content}"
            );
        }
        let mut bad = body();
        bad["doc"]["content"][0]["type"] = json!("script");
        let refusal = ty.validate(&with("body", bad)).expect_err("refused");
        assert!(
            refusal.pointer.starts_with("/body/doc/content/0"),
            "{}",
            refusal.pointer
        );
    }

    #[test]
    fn definitions_are_checked_field_by_field() {
        let field = |extra: Value| {
            let mut definition = json!({"name": "T", "title_field": "t", "fields": [
                {"id": "t", "label": "T", "type": "text", "required": true}
            ]});
            definition["fields"]
                .as_array_mut()
                .expect("a list")
                .push(extra);
            ContentType::parse(&definition)
        };
        let cases = [
            (
                json!({"id": "Bad", "label": "x", "type": "text"}),
                "invalid_id",
                "/fields/1/id",
            ),
            (
                json!({"id": "x", "label": " x", "type": "text"}),
                "invalid_label",
                "/fields/1/label",
            ),
            (
                json!({"id": "x", "label": "x", "type": "image"}),
                "invalid_type",
                "/fields/1/type",
            ),
            (
                json!({"id": "x", "label": "x", "type": "text", "max_chars": 0}),
                "out_of_range",
                "/fields/1/max_chars",
            ),
            (
                json!({"id": "x", "label": "x", "type": "boolean", "max_chars": 3}),
                "unknown_field",
                "/fields/1/max_chars",
            ),
            (
                json!({"id": "x", "label": "x", "type": "number", "min": 5, "max": 1}),
                "out_of_range",
                "/fields/1/max",
            ),
            (
                json!({"id": "x", "label": "x", "type": "enum", "values": ["a", "a"]}),
                "invalid_values",
                "/fields/1/values/1",
            ),
            (
                json!({"id": "t", "label": "x", "type": "text"}),
                "duplicate_field",
                "/fields/1/id",
            ),
        ];
        for (extra, code, pointer) in cases {
            let refusal = field(extra.clone()).expect_err("refused");
            assert_eq!(
                (refusal.code, refusal.pointer.as_str()),
                (code, pointer),
                "{extra}"
            );
        }
        let no_title = ContentType::parse(&json!({"name": "T", "title_field": "t", "fields": [
            {"id": "t", "label": "T", "type": "text"}
        ]}))
        .expect_err("an optional title can't be the title");
        assert_eq!(no_title.code, "invalid_title_field");
    }

    #[test]
    fn safe_changes_are_allowed_and_risky_ones_need_a_migration() {
        let base = post_type();
        let mut definition: Value = serde_json::from_str(&base.canonical_json()).expect("JSON");
        definition["name"] = json!("Stories");
        definition["fields"][0]["label"] = json!("Title");
        definition["fields"][0]["max_chars"] = json!(200);
        definition["fields"][6]["values"] = json!(["News", "Guides", "Opinion"]);
        definition["fields"][3]["integer"] = json!(false);
        definition["fields"]
            .as_array_mut()
            .expect("a list")
            .push(json!({"id": "subtitle", "label": "Subtitle", "type": "text"}));
        let wider = ContentType::parse(&definition).expect("valid");
        assert_eq!(base.successor(&wider), Successor::Compatible);

        let change = |edit: &dyn Fn(&mut Value)| {
            let mut definition: Value = serde_json::from_str(&base.canonical_json()).expect("JSON");
            edit(&mut definition);
            match base.successor(&ContentType::parse(&definition).expect("valid")) {
                Successor::Compatible => "compatible",
                Successor::CheckContent(refusal) => {
                    assert_eq!(refusal.code, "needs_migration");
                    "check content"
                }
                Successor::Refused(refusal) => {
                    assert_eq!(refusal.code, "needs_migration");
                    "refused"
                }
            }
        };
        let narrower: [&dyn Fn(&mut Value); 7] = [
            &|d| d["fields"][0]["max_chars"] = json!(100),
            &|d| d["fields"][1]["required"] = json!(true),
            &|d| d["fields"][6]["values"] = json!(["News"]),
            &|d| d["fields"][4]["integer"] = json!(true),
            &|d| {
                d["fields"].as_array_mut().expect("a list").remove(5);
            },
            &|d| {
                d["fields"]
                    .as_array_mut()
                    .expect("a list")
                    .push(json!({"id": "x", "label": "X", "type": "boolean", "required": true}));
            },
            &|d| d["fields"][5] = json!({"id": "featured", "label": "Featured", "type": "text"}),
        ];
        for edit in narrower {
            assert_eq!(change(edit), "check content");
        }
        let refused: [&dyn Fn(&mut Value); 2] = [
            &|d| {
                d["fields"][1] =
                    json!({"id": "summary", "label": "Summary", "type": "text", "required": true});
                d["title_field"] = json!("summary");
            },
            &|d| d["fields"][6] = json!({"id": "section", "label": "Section", "type": "slug"}),
        ];
        for edit in refused {
            assert_eq!(change(edit), "refused");
        }
    }

    #[test]
    fn a_narrower_definition_accepts_exactly_the_content_that_still_fits() {
        let base = post_type();
        let mut definition: Value = serde_json::from_str(&base.canonical_json()).expect("JSON");
        // Drops `rating`, and makes `summary` required and `section` news-only.
        definition["fields"]
            .as_array_mut()
            .expect("a list")
            .remove(4);
        definition["fields"][1]["required"] = json!(true);
        definition["fields"][5]["values"] = json!(["News"]);
        let next = ContentType::parse(&definition).expect("valid");
        assert!(matches!(base.successor(&next), Successor::CheckContent(_)));

        let stored = |content: Value| {
            base.validate(&content)
                .expect("valid under the old definition")
                .canonical_json()
                .to_owned()
        };
        let fits = stored(json!({"headline": "Hi", "summary": "S", "body": body(), "section": "News"}));
        assert_eq!(next.accepts(&fits), Ok(()));
        let cases = [
            (json!({"headline": "Hi", "summary": "S", "body": body(), "rating": 3}), "unknown_field", "/rating"),
            (json!({"headline": "Hi", "body": body()}), "missing_field", "/summary"),
            (json!({"headline": "Hi", "summary": "S", "body": body(), "section": "Guides"}), "not_an_option", "/section"),
        ];
        for (content, code, pointer) in cases {
            let refusal = next.accepts(&stored(content.clone())).expect_err("in the way");
            assert_eq!((refusal.code, refusal.pointer.as_str()), (code, pointer), "{content}");
        }
        assert_eq!(next.accepts("{").map_err(|refusal| refusal.code), Err("unreadable"));
    }

    #[test]
    fn slugs_are_url_names_and_content_reports_its_slug() {
        for good in ["a", "my-first-post", "2026-10-27", "x1-y2-z3", &"a".repeat(MAX_SLUG_CHARS)] {
            assert!(is_slug(good), "{good}");
        }
        for bad in [
            "",
            "-a",
            "a-",
            "a--b",
            "My-Post",
            "café",
            "a_b",
            "a b",
            "a/b",
            &"a".repeat(MAX_SLUG_CHARS + 1),
        ] {
            assert!(!is_slug(bad), "{bad}");
        }

        let ty = ContentType::parse(&json!({"name": "Pages", "title_field": "title", "fields": [
            {"id": "title", "label": "Title", "type": "text", "required": true},
            {"id": "slug", "label": "URL name", "type": "slug", "required": true, "source": "title"}
        ]}))
        .expect("valid");
        assert!(ty.canonical_json().ends_with(
            r#"{"id":"slug","label":"URL name","type":"slug","required":true,"source":"title"}]}"#
        ));
        let content = ty
            .validate(&json!({"slug": "hello-world", "title": "Hello, world"}))
            .expect("valid");
        assert_eq!(
            content.canonical_json(),
            r#"{"title":"Hello, world","slug":"hello-world"}"#
        );
        assert_eq!(content.slug(), Some("hello-world"));
        let refusal = ty
            .validate(&json!({"title": "T", "slug": "Hello World"}))
            .expect_err("not a slug");
        assert_eq!((refusal.code, refusal.pointer.as_str()), ("invalid_slug", "/slug"));
        let without = ContentType::articles()
            .validate(&json!({"title": "T", "body": body()}))
            .expect("valid");
        assert_eq!(without.slug(), None);
    }

    #[test]
    fn a_slug_field_names_a_real_source_and_comes_once() {
        let parse = |fields: Value| {
            ContentType::parse(&json!({"name": "T", "title_field": "t", "fields": fields}))
                .map(|_| ())
                .map_err(|refusal| (refusal.code, refusal.pointer))
        };
        let title = json!({"id": "t", "label": "T", "type": "text", "required": true});
        assert_eq!(
            parse(json!([title, {"id": "s", "label": "S", "type": "slug"}])),
            Ok(())
        );
        let cases = [
            (
                json!([title, {"id": "s", "label": "S", "type": "slug"}, {"id": "u", "label": "U", "type": "slug"}]),
                ("duplicate_slug", "/fields/2/type"),
            ),
            (
                json!([title, {"id": "s", "label": "S", "type": "slug", "source": "missing"}]),
                ("invalid_source", "/fields/1/source"),
            ),
            (
                json!([title, {"id": "m", "label": "M", "type": "text", "multiline": true},
                       {"id": "s", "label": "S", "type": "slug", "source": "m"}]),
                ("invalid_source", "/fields/2/source"),
            ),
            (
                json!([title, {"id": "s", "label": "S", "type": "slug", "source": 3}]),
                ("invalid_source", "/fields/1/source"),
            ),
            (
                json!([title, {"id": "s", "label": "S", "type": "slug", "max_chars": 10}]),
                ("unknown_field", "/fields/1/max_chars"),
            ),
        ];
        for (fields, expected) in cases {
            assert_eq!(
                parse(fields.clone()),
                Err((expected.0, expected.1.to_owned())),
                "{fields}"
            );
        }
    }

    #[test]
    fn timestamps_are_real_utc_moments() {
        for good in [
            "2026-10-25T14:30:00Z",
            "2000-02-29T00:00:00Z",
            "0001-01-01T00:00:00Z",
        ] {
            assert!(is_utc_timestamp(good), "{good}");
        }
        for bad in [
            "1900-02-29T00:00:00Z",
            "2026-13-01T00:00:00Z",
            "2026-04-31T00:00:00Z",
            "2026-10-25T24:00:00Z",
            "2026-10-25T14:30:00.5Z",
            "2026-10-25 14:30:00Z",
            "0000-01-01T00:00:00Z",
        ] {
            assert!(!is_utc_timestamp(bad), "{bad}");
        }
    }
}
