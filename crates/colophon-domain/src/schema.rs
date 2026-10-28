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
//!
//! Step M.1 (D-064) added email, URL, date, time, JSON, markdown and colour fields, a
//! `description` on any field and a `min_chars` on text. Each new value is written in one form
//! the Studio can reproduce (a URL as WHATWG `href`, JSON with its keys sorted), and the new
//! options are written only when set, so older definitions keep their bytes and their hashes.

use serde_json::{Map, Value};

use crate::rich_text::{self, json::write_string, pointer::Pointer};

/// Longest field description.
pub const MAX_DESCRIPTION_CHARS: usize = 500;
/// Longest email address (RFC 5321's path limit).
pub const MAX_EMAIL_CHARS: usize = 254;
/// Longest URL.
pub const MAX_URL_CHARS: usize = 2048;
/// Largest JSON value, written canonically.
pub const MAX_JSON_BYTES: usize = 64 * 1024;
/// Deepest nesting in a JSON value.
pub const MAX_JSON_DEPTH: usize = 32;
/// Most entries one reference field may name (step M.2).
pub const MAX_REFERENCES: u16 = 100;

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
        /// The fewest characters allowed (0: no minimum; step M.1).
        min_chars: u64,
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
    /// An email address, `local@domain.tld` (step M.1): checked for shape, never for delivery.
    Email,
    /// An `http` or `https` URL without credentials, stored in its WHATWG form (what a browser's
    /// `new URL(text).href` gives), so the Studio writes the same bytes.
    Url,
    /// A calendar date, `YYYY-MM-DD`.
    Date,
    /// A time of day, `HH:MM` or `HH:MM:SS`, 24-hour.
    Time,
    /// Any JSON value, up to [`MAX_JSON_BYTES`] and [`MAX_JSON_DEPTH`]: objects with their keys
    /// sorted, numbers as number fields write them.
    Json,
    /// Markdown source: multi-line text the site renders, never HTML stored here.
    Markdown {
        /// The most characters allowed.
        max_chars: u64,
    },
    /// A colour, `#rrggbb` in lowercase.
    Color,
    /// Entries of another collection (or the same one), by id (step M.2, D-065): one, or a list
    /// of distinct ones. The database refuses an id that names no entry of `to`.
    Reference {
        /// The collection the entries belong to.
        to: String,
        /// A list rather than one.
        many: bool,
        /// The most a list holds (1 to [`MAX_REFERENCES`]; 1 for one).
        max: u16,
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
            Self::Email => "email",
            Self::Url => "url",
            Self::Date => "date",
            Self::Time => "time",
            Self::Json => "json",
            Self::Markdown { .. } => "markdown",
            Self::Color => "color",
            Self::Reference { .. } => "reference",
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
    /// Help shown under it in the editor (empty: none; step M.1).
    pub description: String,
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
    references: Vec<ContentReference>,
}

/// One entry a reference field names, at its place in the field (step M.2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentReference {
    /// The field's id.
    pub field: String,
    /// The collection the entry must belong to.
    pub collection: String,
    /// Its place in a list (0 for a single reference).
    pub position: u16,
    /// The entry's id, a lowercase UUID.
    pub entry: String,
}

impl Content {
    /// Every entry the content's reference fields name, in field order: what the database
    /// checks and keeps (D-065).
    pub fn references(&self) -> &[ContentReference] {
        &self.references
    }

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

pub(crate) fn is_field_id(text: &str) -> bool {
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

/// A field's `description`: empty when absent; otherwise plain text of at most
/// [`MAX_DESCRIPTION_CHARS`].
fn description(object: &Map<String, Value>, pointer: &Pointer) -> Result<String, Refusal> {
    match object.get("description") {
        None => Ok(String::new()),
        Some(value) => Ok(value
            .as_str()
            .filter(|text| text.is_empty() || plain_text(text, MAX_DESCRIPTION_CHARS))
            .ok_or_else(|| {
                refuse(
                    "invalid_description",
                    &at(pointer, "description"),
                    format!(
                        "a description is at most {MAX_DESCRIPTION_CHARS} characters, without control characters or spaces at either end"
                    ),
                )
            })?
            .to_owned()),
    }
}

/// A slug field's `source`: the id of the field the Studio suggests it from, if any.
fn slug_source(object: &Map<String, Value>, pointer: &Pointer) -> Result<Option<String>, Refusal> {
    match object.get("source") {
        None => Ok(None),
        Some(value) => Ok(Some(
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
        )),
    }
}

/// A reference field's target collection, `many` (default false) and, for a list, `max` (1 to
/// [`MAX_REFERENCES`], default [`MAX_REFERENCES`]).
fn reference_kind(object: &Map<String, Value>, pointer: &Pointer) -> Result<FieldKind, Refusal> {
    let to = object
        .get("to")
        .and_then(Value::as_str)
        .filter(|to| crate::project::Slug::parse(to).is_ok())
        .ok_or_else(|| {
            refuse(
                "invalid_target",
                &at(pointer, "to"),
                "`to` is the slug of the collection the entries belong to",
            )
        })?
        .to_owned();
    let many = bool_or(object, "many", false, pointer)?;
    let max = match (many, object.get("max")) {
        (_, None) => {
            if many {
                MAX_REFERENCES
            } else {
                1
            }
        }
        (false, Some(_)) => {
            return Err(refuse(
                "out_of_range",
                &at(pointer, "max"),
                "`max` is for a list of references (`many`)",
            ));
        }
        (true, Some(value)) => value
            .as_u64()
            .and_then(|max| u16::try_from(max).ok())
            .filter(|max| (1..=MAX_REFERENCES).contains(max))
            .ok_or_else(|| {
                refuse(
                    "out_of_range",
                    &at(pointer, "max"),
                    format!("`max` is a whole number from 1 to {MAX_REFERENCES}"),
                )
            })?,
    };
    Ok(FieldKind::Reference { to, many, max })
}

/// A UUID in its canonical spelling: 36 characters, lowercase hexadecimal, hyphens at 8, 13, 18
/// and 23.
fn is_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            8 | 13 | 18 | 23 => *byte == b'-',
            _ => byte.is_ascii_digit() || (b'a'..=b'f').contains(byte),
        })
}

/// The kinds step M.1 added that take no options: their type name, or `None`.
fn simple_kind(name: &str) -> Option<FieldKind> {
    Some(match name {
        "email" => FieldKind::Email,
        "url" => FieldKind::Url,
        "date" => FieldKind::Date,
        "time" => FieldKind::Time,
        "json" => FieldKind::Json,
        "color" => FieldKind::Color,
        _ => return None,
    })
}

/// `local@domain`, as most mail software accepts it: a dot-atom local part of at most 64
/// characters and a host name with at least one dot. A check of shape, never of delivery.
fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    text.len() <= MAX_EMAIL_CHARS
        && (1..=64).contains(&local.len())
        && !local.starts_with('.')
        && !local.ends_with('.')
        && !local.contains("..")
        && local
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"!#$%&'*+-/=?^_`{|}~.".contains(&byte))
        && domain.contains('.')
        && is_host_name(domain)
}

/// Letters, digits and hyphens in dot-separated labels of 1 to 63 characters, at most 253 in all.
fn is_host_name(text: &str) -> bool {
    text.len() <= 253
        && text.split('.').all(|label| {
            (1..=63).contains(&label.len())
                && !label.starts_with('-')
                && !label.ends_with('-')
                && label
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
        })
}

/// The WHATWG form of an `http` or `https` URL with a host and no user name or password, or
/// `None`.
fn web_address(text: &str) -> Option<String> {
    if text.len() > MAX_URL_CHARS || text.chars().any(char::is_whitespace) {
        return None;
    }
    let parsed = url::Url::parse(text).ok()?;
    let fits = matches!(parsed.scheme(), "http" | "https")
        && parsed.host_str().is_some_and(|host| !host.is_empty())
        && parsed.username().is_empty()
        && parsed.password().is_none();
    let href = String::from(parsed);
    (fits && href.len() <= MAX_URL_CHARS).then_some(href)
}

/// `YYYY-MM-DD`, a real calendar date.
fn is_date(text: &str) -> bool {
    text.len() == 10 && is_utc_timestamp(&format!("{text}T00:00:00Z"))
}

/// `HH:MM` or `HH:MM:SS`, 24-hour.
fn is_time(text: &str) -> bool {
    match text.len() {
        5 => is_utc_timestamp(&format!("2000-01-01T{text}:00Z")),
        8 => is_utc_timestamp(&format!("2000-01-01T{text}Z")),
        _ => false,
    }
}

/// `#rrggbb` in lowercase.
fn is_color(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
}

/// Why a JSON field's value was refused.
enum JsonProblem {
    /// Written canonically, it passes [`MAX_JSON_BYTES`].
    TooLarge,
    /// Too deep, or a number out of range.
    Invalid(String),
}

impl From<String> for JsonProblem {
    fn from(message: String) -> Self {
        Self::Invalid(message)
    }
}

/// Writes a JSON field's value canonically: object keys sorted (by UTF-16 code units, as
/// JavaScript sorts them), strings as `JSON.stringify` escapes them, numbers as number fields
/// write them (whole numbers in the safe range, or decimals with at most six places). `start` is
/// where the value began in `out`, so its size is its own.
fn write_json(
    value: &Value,
    depth: usize,
    out: &mut String,
    start: usize,
) -> Result<(), JsonProblem> {
    if depth > MAX_JSON_DEPTH {
        return Err(JsonProblem::Invalid(format!(
            "JSON nests at most {MAX_JSON_DEPTH} levels deep"
        )));
    }
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => {
            let float = number
                .as_f64()
                .ok_or_else(|| "a number in JSON is too large".to_owned())?;
            let integer = number.is_i64() || number.is_u64() || float.fract() == 0.0;
            check_number(float, integer)?;
            write_number(out, float, integer);
        }
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(item, depth + 1, out, start)?;
                if out.len() - start > MAX_JSON_BYTES {
                    return Err(JsonProblem::TooLarge);
                }
            }
            out.push(']');
        }
        Value::Object(object) => {
            let mut keys: Vec<&String> = object.keys().collect();
            keys.sort_by(|one, other| one.encode_utf16().cmp(other.encode_utf16()));
            out.push('{');
            for (index, key) in keys.into_iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_json(&object[key], depth + 1, out, start)?;
                if out.len() - start > MAX_JSON_BYTES {
                    return Err(JsonProblem::TooLarge);
                }
            }
            out.push('}');
        }
    }
    Ok(())
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
    let min_chars = match object.get("min_chars") {
        None => 0,
        Some(value) => value
            .as_u64()
            .filter(|min| (1..=max_chars).contains(min))
            .ok_or_else(|| {
                refuse(
                    "out_of_range",
                    &at(pointer, "min_chars"),
                    "`min_chars` is a whole number from 1 to `max_chars`",
                )
            })?,
    };
    Ok(FieldKind::Text {
        min_chars,
        max_chars,
        multiline: bool_or(object, "multiline", false, pointer)?,
    })
}

/// A markdown field's `max_chars` (default 100,000).
fn markdown_kind(object: &Map<String, Value>, pointer: &Pointer) -> Result<FieldKind, Refusal> {
    let max_chars = match object.get("max_chars") {
        None => MAX_TEXT_CHARS,
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
    Ok(FieldKind::Markdown { max_chars })
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
                min_chars: old_min,
                max_chars: old_max,
                multiline: old_multiline,
            },
            FieldKind::Text {
                min_chars: new_min,
                max_chars: new_max,
                multiline: new_multiline,
            },
        ) => new_min <= old_min && new_max >= old_max && (*new_multiline || !old_multiline),
        // Markdown accepts any text that fits, line breaks included.
        (
            FieldKind::Text {
                max_chars: old_max, ..
            }
            | FieldKind::Markdown { max_chars: old_max },
            FieldKind::Markdown { max_chars: new_max },
        ) => new_max >= old_max,
        (FieldKind::RichText, FieldKind::RichText)
        | (FieldKind::Boolean, FieldKind::Boolean)
        | (FieldKind::DateTime, FieldKind::DateTime)
        | (FieldKind::Slug { .. }, FieldKind::Slug { .. })
        | (FieldKind::Email, FieldKind::Email)
        | (FieldKind::Url, FieldKind::Url)
        | (FieldKind::Date, FieldKind::Date)
        | (FieldKind::Time, FieldKind::Time)
        | (FieldKind::Json, FieldKind::Json)
        | (FieldKind::Color, FieldKind::Color) => true,
        (
            FieldKind::Reference {
                to: old_to,
                many: old_many,
                max: old_max,
            },
            FieldKind::Reference {
                to: new_to,
                many: new_many,
                max: new_max,
            },
        ) => old_to == new_to && old_many == new_many && new_max >= old_max,
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

/// The slug rules of a definition (D-054): at most one slug field, and a `source` that names one of
/// the definition's single-line text fields.
fn check_slug_fields(fields: &[Field]) -> Result<(), Refusal> {
    let pointer_to = |index: usize, key: &str| {
        let mut pointer = at(&Pointer::new(), "fields");
        pointer.push_index(index);
        at(&pointer, key)
    };
    let mut slugs = fields
        .iter()
        .enumerate()
        .filter(|(_, field)| matches!(field.kind, FieldKind::Slug { .. }));
    if let (Some(_), Some((second, _))) = (slugs.next(), slugs.next()) {
        return Err(refuse(
            "duplicate_slug",
            &pointer_to(second, "type"),
            "a content type has at most one slug field",
        ));
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
            return Err(refuse(
                "invalid_source",
                &pointer_to(index, "source"),
                "`source` must name a single-line text field of this content type",
            ));
        }
    }
    Ok(())
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
                    description: String::new(),
                    required: true,
                    kind: FieldKind::Text {
                        min_chars: 0,
                        max_chars: 200,
                        multiline: false,
                    },
                },
                Field {
                    id: "body".to_owned(),
                    label: "Body".to_owned(),
                    description: String::new(),
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
            fields.push(field);
        }
        check_slug_fields(&fields)?;
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
        let description = description(object, pointer)?;
        let common = ["id", "label", "description", "type", "required"];
        let kind = Self::kind(object, &common, pointer)?;
        Ok(Field {
            id,
            label,
            description,
            required,
            kind,
        })
    }

    /// What a field holds: its `type` and that type's options, refusing any other key.
    fn kind(
        object: &Map<String, Value>,
        common: &[&str],
        pointer: &Pointer,
    ) -> Result<FieldKind, Refusal> {
        Ok(match object.get("type").and_then(Value::as_str) {
            Some("text") => {
                only(
                    object,
                    &[common, &["min_chars", "max_chars", "multiline"]].concat(),
                    pointer,
                )?;
                text_kind(object, pointer)?
            }
            Some("rich_text") => {
                only(object, common, pointer)?;
                FieldKind::RichText
            }
            Some("number") => {
                only(
                    object,
                    &[common, &["integer", "min", "max"]].concat(),
                    pointer,
                )?;
                number_kind(object, pointer)?
            }
            Some("boolean") => {
                only(object, common, pointer)?;
                FieldKind::Boolean
            }
            Some("enum") => {
                only(object, &[common, &["values"]].concat(), pointer)?;
                FieldKind::Enum {
                    values: enum_values(object, pointer)?,
                }
            }
            Some("datetime") => {
                only(object, common, pointer)?;
                FieldKind::DateTime
            }
            Some("slug") => {
                only(object, &[common, &["source"]].concat(), pointer)?;
                FieldKind::Slug {
                    source: slug_source(object, pointer)?,
                }
            }
            Some(name) if simple_kind(name).is_some() => {
                only(object, common, pointer)?;
                simple_kind(name).unwrap_or(FieldKind::Json)
            }
            Some("markdown") => {
                only(object, &[common, &["max_chars"]].concat(), pointer)?;
                markdown_kind(object, pointer)?
            }
            Some("reference") => {
                only(object, &[common, &["to", "many", "max"]].concat(), pointer)?;
                reference_kind(object, pointer)?
            }
            _ => {
                return Err(refuse(
                    "invalid_type",
                    &at(pointer, "type"),
                    "`type` is one of text, rich_text, number, boolean, enum, datetime, slug, email, url, date, time, json, markdown, color, reference",
                ));
            }
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
            // Only when set, so a definition written before step M.1 keeps its bytes and hash.
            if !field.description.is_empty() {
                out.push_str(r#","description":"#);
                write_string(&mut out, &field.description);
            }
            out.push_str(r#","type":"#);
            write_string(&mut out, field.kind.name());
            out.push_str(if field.required {
                r#","required":true"#
            } else {
                r#","required":false"#
            });
            match &field.kind {
                FieldKind::Text {
                    min_chars,
                    max_chars,
                    multiline,
                } => {
                    if *min_chars > 0 {
                        out.push_str(r#","min_chars":"#);
                        out.push_str(&min_chars.to_string());
                    }
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
                FieldKind::Markdown { max_chars } => {
                    out.push_str(r#","max_chars":"#);
                    out.push_str(&max_chars.to_string());
                }
                FieldKind::Reference { to, many, max } => {
                    out.push_str(r#","to":"#);
                    write_string(&mut out, to);
                    if *many {
                        out.push_str(r#","many":true,"max":"#);
                        out.push_str(&max.to_string());
                    } else {
                        out.push_str(r#","many":false"#);
                    }
                }
                FieldKind::RichText
                | FieldKind::Boolean
                | FieldKind::DateTime
                | FieldKind::Email
                | FieldKind::Url
                | FieldKind::Date
                | FieldKind::Time
                | FieldKind::Json
                | FieldKind::Color => {}
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
        let mut references = Vec::new();
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
            if let FieldKind::Reference { to, .. } = &field.kind {
                let ids = value
                    .as_array()
                    .map_or_else(|| vec![value], |list| list.iter().collect());
                for (position, id) in ids.into_iter().enumerate() {
                    references.push(ContentReference {
                        field: field.id.clone(),
                        collection: to.clone(),
                        position: u16::try_from(position).unwrap_or(u16::MAX),
                        entry: id.as_str().unwrap_or_default().to_owned(),
                    });
                }
            }
        }
        out.push('}');
        Ok(Content {
            canonical: out,
            title,
            slug,
            references,
        })
    }

    /// A reference value: one entry id, or (`many`) a list of 1 to `max` distinct ones. Whether
    /// they name entries of the target collection is the database's to check (D-065).
    fn reference(
        label: &str,
        value: &Value,
        (many, max): (bool, u16),
        pointer: &Pointer,
        out: &mut String,
    ) -> Result<(), Refusal> {
        let id = |value: &Value, pointer: &Pointer| -> Result<String, Refusal> {
            value
                .as_str()
                .filter(|text| is_uuid(text))
                .map(str::to_owned)
                .ok_or_else(|| {
                    refuse(
                        "invalid_reference",
                        pointer,
                        format!("{label} names an entry by its id, in lowercase"),
                    )
                })
        };
        if !many {
            write_string(out, &id(value, pointer)?);
            return Ok(());
        }
        let list = value.as_array().ok_or_else(|| {
            refuse(
                "wrong_type",
                pointer,
                format!("{label} is a list of entries"),
            )
        })?;
        if list.is_empty() {
            return Err(refuse(
                "invalid_reference",
                pointer,
                format!("{label} is empty; leave it out instead"),
            ));
        }
        if list.len() > usize::from(max) {
            return Err(refuse(
                "too_many",
                pointer,
                format!("{label} names at most {max} entries"),
            ));
        }
        let mut seen: Vec<String> = Vec::with_capacity(list.len());
        out.push('[');
        for (index, item) in list.iter().enumerate() {
            let mut at = pointer.clone();
            at.push_index(index);
            let entry = id(item, &at)?;
            if seen.contains(&entry) {
                return Err(refuse(
                    "duplicate_reference",
                    &at,
                    format!("{label} names this entry twice"),
                ));
            }
            if index > 0 {
                out.push(',');
            }
            write_string(out, &entry);
            seen.push(entry);
        }
        out.push(']');
        Ok(())
    }

    /// A JSON value, written canonically within [`MAX_JSON_BYTES`] and [`MAX_JSON_DEPTH`].
    fn json(
        label: &str,
        value: &Value,
        pointer: &Pointer,
        out: &mut String,
    ) -> Result<(), Refusal> {
        let start = out.len();
        let written = write_json(value, 0, out, start);
        if matches!(written, Err(JsonProblem::TooLarge)) || out.len() - start > MAX_JSON_BYTES {
            return Err(refuse(
                "too_long",
                pointer,
                format!("{label} is at most {} KiB of JSON", MAX_JSON_BYTES / 1024),
            ));
        }
        if let Err(JsonProblem::Invalid(message)) = written {
            return Err(refuse(
                "invalid_json",
                pointer,
                format!("{label}: {message}"),
            ));
        }
        Ok(())
    }

    /// A number value, whole if `integer`, within `(min, max)`.
    fn number(
        label: &str,
        value: &Value,
        integer: bool,
        (min, max): (Option<f64>, Option<f64>),
        pointer: &Pointer,
        out: &mut String,
    ) -> Result<(), Refusal> {
        let number = value
            .as_f64()
            .ok_or_else(|| refuse("wrong_type", pointer, format!("{label} is a number")))?;
        check_number(number, integer).map_err(|message| {
            refuse(
                if integer {
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
        write_number(out, number, integer);
        Ok(())
    }

    /// A text or markdown value: within `(min, max)` characters, with line breaks only if
    /// `multiline`.
    fn text(
        label: &str,
        value: &Value,
        (min_chars, max_chars): (u64, u64),
        multiline: bool,
        pointer: &Pointer,
        out: &mut String,
    ) -> Result<(), Refusal> {
        let text = value
            .as_str()
            .ok_or_else(|| refuse("wrong_type", pointer, format!("{label} is text")))?;
        check_text(text, label, max_chars, multiline, pointer)?;
        if u64::try_from(text.chars().count()).unwrap_or(u64::MAX) < min_chars {
            return Err(refuse(
                "too_short",
                pointer,
                format!("{label} is at least {min_chars} characters"),
            ));
        }
        write_string(out, text);
        Ok(())
    }

    /// A value of one of the kinds that hold a string in a fixed format (step M.1), written
    /// canonically.
    fn formatted(
        kind: &FieldKind,
        label: &str,
        value: &Value,
        pointer: &Pointer,
        out: &mut String,
    ) -> Result<(), Refusal> {
        let text = value.as_str();
        let (code, example, canonical) = match kind {
            FieldKind::Email => (
                "invalid_email",
                "an email address like name@example.com",
                text.filter(|text| is_email(text)).map(str::to_owned),
            ),
            FieldKind::Url => (
                "invalid_url",
                "a web address starting with https:// or http://",
                text.and_then(web_address),
            ),
            FieldKind::Date => (
                "invalid_date",
                "a date like 2026-10-28",
                text.filter(|text| is_date(text)).map(str::to_owned),
            ),
            FieldKind::Time => (
                "invalid_time",
                "a time like 14:30 or 14:30:15",
                text.filter(|text| is_time(text)).map(str::to_owned),
            ),
            _ => (
                "invalid_color",
                "a colour like #1a2b3c, in lowercase",
                text.filter(|text| is_color(text)).map(str::to_owned),
            ),
        };
        let canonical =
            canonical.ok_or_else(|| refuse(code, pointer, format!("{label} is {example}")))?;
        write_string(out, &canonical);
        Ok(())
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
                min_chars,
                max_chars,
                multiline,
            } => Self::text(
                label,
                value,
                (*min_chars, *max_chars),
                *multiline,
                pointer,
                out,
            )?,
            FieldKind::Markdown { max_chars } => {
                Self::text(label, value, (0, *max_chars), true, pointer, out)?;
            }
            FieldKind::Email
            | FieldKind::Url
            | FieldKind::Date
            | FieldKind::Time
            | FieldKind::Color => Self::formatted(&field.kind, label, value, pointer, out)?,
            FieldKind::Json => Self::json(label, value, pointer, out)?,
            FieldKind::Reference { many, max, .. } => {
                Self::reference(label, value, (*many, *max), pointer, out)?;
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
                Self::number(label, value, *integer, (*min, *max), pointer, out)?;
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
        let target = |field: &Field| match &field.kind {
            FieldKind::Reference { to, .. } => Some(to.clone()),
            _ => None,
        };
        for (index, new) in next.fields.iter().enumerate() {
            if let Some(old) = self.fields.iter().find(|old| old.id == new.id)
                && target(old).is_some()
                && target(old) != target(new)
            {
                let mut pointer = at(&root, "fields");
                pointer.push_index(index);
                return Successor::Refused(refuse(
                    "needs_migration",
                    &pointer,
                    format!(
                        "pointing `{}` at other entries needs a migration: what it stores names entries of its collection",
                        new.id
                    ),
                ));
            }
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
        let fits =
            stored(json!({"headline": "Hi", "summary": "S", "body": body(), "section": "News"}));
        assert_eq!(next.accepts(&fits), Ok(()));
        let cases = [
            (
                json!({"headline": "Hi", "summary": "S", "body": body(), "rating": 3}),
                "unknown_field",
                "/rating",
            ),
            (
                json!({"headline": "Hi", "body": body()}),
                "missing_field",
                "/summary",
            ),
            (
                json!({"headline": "Hi", "summary": "S", "body": body(), "section": "Guides"}),
                "not_an_option",
                "/section",
            ),
        ];
        for (content, code, pointer) in cases {
            let refusal = next
                .accepts(&stored(content.clone()))
                .expect_err("in the way");
            assert_eq!(
                (refusal.code, refusal.pointer.as_str()),
                (code, pointer),
                "{content}"
            );
        }
        assert_eq!(
            next.accepts("{").map_err(|refusal| refusal.code),
            Err("unreadable")
        );
    }

    #[test]
    fn slugs_are_url_names_and_content_reports_its_slug() {
        let longest = "a".repeat(MAX_SLUG_CHARS);
        let too_long = "a".repeat(MAX_SLUG_CHARS + 1);
        for good in [
            "a",
            "my-first-post",
            "2026-10-27",
            "x1-y2-z3",
            longest.as_str(),
        ] {
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
            too_long.as_str(),
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
        assert_eq!(
            (refusal.code, refusal.pointer.as_str()),
            ("invalid_slug", "/slug")
        );
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

    /// Every kind step M.1 added, one optional field each, with a description on one.
    fn profile_type() -> ContentType {
        ContentType::parse(&json!({
            "name": "Profiles",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true, "min_chars": 2, "max_chars": 80},
                {"id": "email", "label": "Email", "description": "Shown to visitors.", "type": "email"},
                {"id": "website", "label": "Website", "type": "url"},
                {"id": "born", "label": "Born", "type": "date"},
                {"id": "opens", "label": "Opens", "type": "time"},
                {"id": "settings", "label": "Settings", "type": "json"},
                {"id": "notes", "label": "Notes", "type": "markdown", "max_chars": 200},
                {"id": "accent", "label": "Accent", "type": "color"}
            ]
        }))
        .expect("a valid definition")
    }

    fn refusal(content: &Value) -> (&'static str, String) {
        let refused = profile_type().validate(content).expect_err("refused");
        (refused.code, refused.pointer)
    }

    #[test]
    fn the_new_kinds_store_one_canonical_form() {
        let content = profile_type()
            .validate(&json!({
                "name": "Ada",
                "email": "ada.lovelace+notes@example.co.uk",
                "website": "HTTPS://Example.COM",
                "born": "2028-02-29",
                "opens": "09:30",
                "settings": {"theme": {"dark": true, "size": 1.5}, "alpha": [1, null, "x"]},
                "notes": "# Notes\n\n- one\n- two",
                "accent": "#1a2b3c"
            }))
            .expect("valid");
        assert_eq!(
            content.canonical_json(),
            r##"{"name":"Ada","email":"ada.lovelace+notes@example.co.uk","website":"https://example.com/","born":"2028-02-29","opens":"09:30","settings":{"alpha":[1,null,"x"],"theme":{"dark":true,"size":1.5}},"notes":"# Notes\n\n- one\n- two","accent":"#1a2b3c"}"##
        );
        // Stored content validates to the same bytes again.
        let again: Value = serde_json::from_str(content.canonical_json()).expect("JSON");
        assert_eq!(
            profile_type()
                .validate(&again)
                .expect("valid")
                .canonical_json(),
            content.canonical_json()
        );
    }

    #[test]
    fn values_of_the_new_kinds_that_do_not_fit_are_refused() {
        for (field, value, code) in [
            ("name", json!("A"), "too_short"),
            ("email", json!("ada@localhost"), "invalid_email"),
            ("email", json!("ada..l@example.com"), "invalid_email"),
            ("email", json!("@example.com"), "invalid_email"),
            ("email", json!("ada @example.com"), "invalid_email"),
            ("website", json!("ftp://example.com"), "invalid_url"),
            ("website", json!("javascript:alert(1)"), "invalid_url"),
            (
                "website",
                json!("https://user:secret@example.com"),
                "invalid_url",
            ),
            ("website", json!("https://example.com/a b"), "invalid_url"),
            ("born", json!("2026-02-29"), "invalid_date"),
            ("born", json!("2026-1-1"), "invalid_date"),
            ("opens", json!("24:00"), "invalid_time"),
            ("opens", json!("9:30"), "invalid_time"),
            ("accent", json!("#1A2B3C"), "invalid_color"),
            ("accent", json!("#123"), "invalid_color"),
            ("settings", json!({"x": 1e30}), "invalid_json"),
            ("notes", json!("bell \u{7}"), "invalid_text"),
            ("notes", json!("x".repeat(201)), "too_long"),
        ] {
            let mut content = json!({"name": "Ada"});
            content[field] = value.clone();
            assert_eq!(
                refusal(&content),
                (code, format!("/{field}")),
                "{field} = {value}"
            );
        }
        let mut deep = json!(0);
        for _ in 0..=MAX_JSON_DEPTH {
            deep = json!([deep]);
        }
        assert_eq!(
            refusal(&json!({"name": "Ada", "settings": deep})).0,
            "invalid_json"
        );
        let big: Vec<String> = (0..10_000).map(|n| format!("item {n}")).collect();
        assert_eq!(
            refusal(&json!({"name": "Ada", "settings": big})).0,
            "too_long"
        );
    }

    #[test]
    fn definitions_written_before_the_new_kinds_keep_their_bytes() {
        // A definition with none of step M.1's options is written exactly as before, so its
        // hash, which publish requests pin, doesn't move.
        assert!(
            post_type().canonical_json().starts_with(
                r#"{"name":"Posts","title_field":"headline","fields":[{"id":"headline","label":"Headline","type":"text","required":true,"max_chars":120,"multiline":false}"#
            )
        );
        let canonical = profile_type().canonical_json();
        assert!(canonical.contains(r#"{"id":"name","label":"Name","type":"text","required":true,"min_chars":2,"max_chars":80,"multiline":false}"#), "{canonical}");
        assert!(canonical.contains(r#"{"id":"email","label":"Email","description":"Shown to visitors.","type":"email","required":false}"#), "{canonical}");
        assert!(canonical.contains(r#"{"id":"notes","label":"Notes","type":"markdown","required":false,"max_chars":200}"#), "{canonical}");
        let again =
            ContentType::parse(&serde_json::from_str(&canonical).expect("JSON")).expect("valid");
        assert_eq!(again.canonical_json(), canonical);
    }

    #[test]
    fn the_new_options_are_checked_where_they_are_defined() {
        let field = |extra: Value| {
            let mut field = json!({"id": "x", "label": "X", "type": "text"});
            for (key, value) in extra.as_object().expect("object") {
                field[key] = value.clone();
            }
            ContentType::parse(&json!({
                "name": "T", "title_field": "t",
                "fields": [{"id": "t", "label": "T", "type": "text", "required": true}, field]
            }))
            .map(|_| ())
            .map_err(|refused| (refused.code, refused.pointer))
        };
        assert_eq!(
            field(json!({"min_chars": 300})),
            Err(("out_of_range", "/fields/1/min_chars".to_owned()))
        );
        assert_eq!(
            field(json!({"min_chars": 0})),
            Err(("out_of_range", "/fields/1/min_chars".to_owned()))
        );
        assert_eq!(
            field(json!({"description": " padded "})),
            Err(("invalid_description", "/fields/1/description".to_owned()))
        );
        assert_eq!(
            field(json!({"type": "email", "max_chars": 10})),
            Err(("unknown_field", "/fields/1/max_chars".to_owned()))
        );
        assert_eq!(
            field(json!({"type": "markdown", "max_chars": 0})),
            Err(("out_of_range", "/fields/1/max_chars".to_owned()))
        );
        assert_eq!(
            field(json!({"type": "colour"})).map_err(|e| e.0),
            Err("invalid_type")
        );
        assert_eq!(field(json!({"description": ""})), Ok(()));
    }

    #[test]
    fn changes_to_the_new_kinds_follow_the_same_rules() {
        let with = |field: Value| {
            ContentType::parse(&json!({
                "name": "T", "title_field": "t",
                "fields": [{"id": "t", "label": "T", "type": "text", "required": true}, field]
            }))
            .expect("valid")
        };
        let text = with(
            json!({"id": "x", "label": "X", "type": "text", "multiline": true, "max_chars": 500}),
        );
        let markdown =
            with(json!({"id": "x", "label": "X", "type": "markdown", "max_chars": 1000}));
        let email = with(json!({"id": "x", "label": "X", "type": "email"}));
        let picky = with(json!({"id": "x", "label": "X", "type": "text", "min_chars": 5}));
        let plain = with(json!({"id": "x", "label": "X", "type": "text"}));
        assert_eq!(
            text.successor(&markdown),
            Successor::Compatible,
            "markdown holds any text that fits"
        );
        assert!(matches!(
            markdown.successor(&text),
            Successor::CheckContent(_)
        ));
        assert!(
            matches!(text.successor(&email), Successor::CheckContent(_)),
            "text becomes email only if every stored value is one"
        );
        assert!(
            matches!(plain.successor(&picky), Successor::CheckContent(_)),
            "a new minimum needs the content checked"
        );
        assert_eq!(picky.successor(&plain), Successor::Compatible);
        assert_eq!(email.successor(&email), Successor::Compatible);
    }

    fn story_type(tags: &Value) -> ContentType {
        ContentType::parse(&json!({
            "name": "Stories",
            "title_field": "title",
            "fields": [
                {"id": "title", "label": "Title", "type": "text", "required": true},
                {"id": "author", "label": "Author", "type": "reference", "to": "people"},
                tags
            ]
        }))
        .expect("a valid definition")
    }

    const A: &str = "01a18f43-a955-755e-ab9c-1ded4406683c";
    const B: &str = "01a18f43-a955-755e-ab9c-1ded4406683d";

    #[test]
    fn references_name_entries_by_id_and_report_them_in_order() {
        let stories = story_type(
            &json!({"id": "tags", "label": "Tags", "type": "reference", "to": "tags", "many": true, "max": 3}),
        );
        let content = stories
            .validate(&json!({"title": "Hi", "author": A, "tags": [B, A]}))
            .expect("valid");
        assert_eq!(
            content.canonical_json(),
            format!(r#"{{"title":"Hi","author":"{A}","tags":["{B}","{A}"]}}"#)
        );
        let named: Vec<(&str, &str, u16, &str)> = content
            .references()
            .iter()
            .map(|r| {
                (
                    r.field.as_str(),
                    r.collection.as_str(),
                    r.position,
                    r.entry.as_str(),
                )
            })
            .collect();
        assert_eq!(
            named,
            [
                ("author", "people", 0, A),
                ("tags", "tags", 0, B),
                ("tags", "tags", 1, A)
            ]
        );
        assert!(stories.canonical_json().contains(r#"{"id":"author","label":"Author","type":"reference","required":false,"to":"people","many":false}"#));
        assert!(
            stories.canonical_json().contains(
                r#""type":"reference","required":false,"to":"tags","many":true,"max":3}"#
            )
        );

        for (field, value, code) in [
            ("author", json!(A.to_uppercase()), "invalid_reference"),
            ("author", json!("not-an-id"), "invalid_reference"),
            ("author", json!([A]), "invalid_reference"),
            ("tags", json!(A), "wrong_type"),
            ("tags", json!([]), "invalid_reference"),
            ("tags", json!([A, A]), "duplicate_reference"),
            (
                "tags",
                json!([
                    A,
                    B,
                    "01a18f43-a955-755e-ab9c-1ded4406683e",
                    "01a18f43-a955-755e-ab9c-1ded4406683f"
                ]),
                "too_many",
            ),
        ] {
            let mut content = json!({"title": "Hi"});
            content[field] = value.clone();
            let refused = stories.validate(&content).expect_err("refused");
            assert_eq!(refused.code, code, "{field} = {value}");
        }
    }

    #[test]
    fn a_reference_field_names_its_collection_and_its_limits() {
        let parse = |field: Value| {
            ContentType::parse(&json!({
                "name": "T", "title_field": "t",
                "fields": [{"id": "t", "label": "T", "type": "text", "required": true}, field]
            }))
            .map(|_| ())
            .map_err(|refused| (refused.code, refused.pointer))
        };
        let base = |extra: Value| {
            let mut field = json!({"id": "x", "label": "X", "type": "reference", "to": "tags"});
            for (key, value) in extra.as_object().expect("object") {
                field[key] = value.clone();
            }
            parse(field)
        };
        assert_eq!(
            base(json!({"to": "Not A Slug"})),
            Err(("invalid_target", "/fields/1/to".to_owned()))
        );
        assert_eq!(
            base(json!({"max": 5})),
            Err(("out_of_range", "/fields/1/max".to_owned()))
        );
        assert_eq!(
            base(json!({"many": true, "max": 0})),
            Err(("out_of_range", "/fields/1/max".to_owned()))
        );
        assert_eq!(
            base(json!({"many": true, "max": 101})),
            Err(("out_of_range", "/fields/1/max".to_owned()))
        );
        assert_eq!(base(json!({"many": true})), Ok(()));
    }

    #[test]
    fn pointing_a_reference_elsewhere_needs_a_migration() {
        let with = |field: Value| story_type(&field);
        let tags = |max: u16| {
            with(
                json!({"id": "tags", "label": "Tags", "type": "reference", "to": "tags", "many": true, "max": max}),
            )
        };
        let topics = with(
            json!({"id": "tags", "label": "Tags", "type": "reference", "to": "topics", "many": true, "max": 3}),
        );
        let single =
            with(json!({"id": "tags", "label": "Tags", "type": "reference", "to": "tags"}));
        assert!(
            matches!(tags(3).successor(&topics), Successor::Refused(_)),
            "other entries entirely"
        );
        assert_eq!(tags(3).successor(&tags(5)), Successor::Compatible);
        assert!(matches!(
            tags(5).successor(&tags(3)),
            Successor::CheckContent(_)
        ));
        assert!(
            matches!(single.successor(&tags(3)), Successor::CheckContent(_)),
            "one becomes a list only if nothing is stored"
        );
    }
}
