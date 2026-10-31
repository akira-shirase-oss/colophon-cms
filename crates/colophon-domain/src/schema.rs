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
//! ([`ContentType::check_successor`]): new optional fields, wider limits, more enum options, new
//! labels. Anything else needs a migration, which doesn't exist yet.

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
/// The most bytes a `json` field's canonical value may occupy.
pub const MAX_JSON_BYTES: u64 = 65_536;
/// The deepest a `json` field's value may nest.
pub const MAX_JSON_DEPTH: u32 = 32;
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
    /// A day, written `YYYY-MM-DD`.
    Date,
    /// A time of day, written `HH:MM` or `HH:MM:SS`.
    Time,
    /// An email address, checked as one.
    Email,
    /// A web address: `http:` or `https:` only, no credentials.
    Url,
    /// Text written in Markdown, at most `max_chars` characters; line breaks and tabs allowed,
    /// other control characters not.
    Markdown {
        /// The most characters allowed.
        max_chars: u64,
    },
    /// A colour, written `#rrggbb` (lowercase hex).
    Color,
    /// Any JSON value bounded in depth and bytes, written canonical with keys sorted and
    /// numbers held to the project's decimal model ([`MAX_DECIMAL`], [`DECIMAL_PLACES`]).
    Json {
        /// The most bytes the canonical value may occupy.
        max_bytes: u64,
    },
    /// An object shaped by its own list of fields. Fields inside an object carry `required`
    /// and their kind's options only — `default`, `when` and `unique` live on the object
    /// field itself. An object can't hold another object yet.
    Object {
        /// The fields the object's value is shaped by.
        fields: Vec<Field>,
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
            Self::Date => "date",
            Self::Time => "time",
            Self::Email => "email",
            Self::Url => "url",
            Self::Markdown { .. } => "markdown",
            Self::Color => "color",
            Self::Json { .. } => "json",
            Self::Object { .. } => "object",
        }
    }
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
    /// What an entry gets when the field is left out, as canonical JSON; `None` unless set.
    /// Checked against the kind when the definition is parsed.
    pub default: Option<String>,
    /// When an editor shows the field, decided by another field's value; `None` means always.
    /// Display-only: a stored value is checked the same whether or not the field would show,
    /// and a required field can't hide.
    pub when: Option<When>,
    /// Whether no two entries in the collection may share its value. Enforced by the
    /// `unique_values` index in the database (migration 0016), kept to kinds whose values are
    /// single and short.
    pub unique: bool,
}

/// The values of another field under which this field shows.
#[derive(Debug, Clone, PartialEq)]
pub struct When {
    /// The other field's id.
    pub field: String,
    /// The values that show it, compared as strings (`"true"` for a checked yes/no).
    pub options: Vec<String>,
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
    /// Each written field's id and its canonical spelling, in definition order.
    values: Vec<(String, String)>,
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

    /// A written field's canonical spelling, by field id (`None` when the field was left out).
    pub fn value(&self, field_id: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(id, _)| id == field_id)
            .map(|(_, spelling)| spelling.as_str())
    }
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

/// A day, `YYYY-MM-DD`: the date part of a UTC timestamp's shape and ranges.
fn is_date(text: &str) -> bool {
    let bytes = text.as_bytes();
    let shape = bytes.len() == 10
        && bytes.iter().enumerate().all(|(index, byte)| match index {
            4 | 7 => *byte == b'-',
            _ => byte.is_ascii_digit(),
        });
    if !shape {
        return false;
    }
    is_utc_timestamp(&format!("{text}T00:00:00Z"))
}

/// A time of day, `HH:MM` or `HH:MM:SS`.
fn is_time(text: &str) -> bool {
    let bytes = text.as_bytes();
    if !(bytes.len() == 5 || bytes.len() == 8)
        || !bytes.iter().enumerate().all(|(index, byte)| match index {
            2 | 5 => *byte == b':',
            _ => byte.is_ascii_digit(),
        })
    {
        return false;
    }
    let number = |range: std::ops::Range<usize>| -> u32 {
        text.get(range)
            .and_then(|digits| digits.parse().ok())
            .unwrap_or(u32::MAX)
    };
    number(0..2) < 24 && number(3..5) < 60 && (bytes.len() == 5 || number(6..8) < 60)
}

/// An email address: one `@`, a non-empty local part and a domain with a dot, no spaces or
/// controls.
fn is_email(text: &str) -> bool {
    let Some((local, domain)) = text.split_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.contains('@')
        && domain.contains('.')
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && domain.split('.').all(|part| !part.is_empty())
        && text
            .chars()
            .all(|c| c.is_ascii() && !c.is_control() && c != ' ')
}

/// A colour: `#` then six lowercase hex digits.
fn is_color(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 7
        && bytes[0] == b'#'
        && bytes[1..]
            .iter()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// A web address: `http:` or `https:`, a non-empty host, no credentials, no spaces or controls.
fn is_url(text: &str) -> bool {
    let Some(rest) = text
        .strip_prefix("https://")
        .or_else(|| text.strip_prefix("http://"))
    else {
        return false;
    };
    let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
    !host.is_empty()
        && !host.contains('@')
        && text
            .chars()
            .all(|c| c.is_ascii() && !c.is_control() && c != ' ')
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

/// A JSON value's canonical form: keys sorted, numbers in the project's decimal model. Depth is
/// bounded so storing one can never blow a stack.
fn write_json(
    value: &Value,
    out: &mut String,
    depth: u32,
    pointer: &Pointer,
    label: &str,
) -> Result<(), Refusal> {
    if depth > MAX_JSON_DEPTH {
        return Err(refuse(
            "too_deep",
            pointer,
            format!("{label} is nested more than {MAX_JSON_DEPTH} levels"),
        ));
    }
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(flag) => out.push_str(if *flag { "true" } else { "false" }),
        Value::Number(number) => {
            if let Some(whole) = number.as_i64() {
                out.push_str(&whole.to_string());
            } else if let Some(whole) = number.as_u64() {
                out.push_str(&whole.to_string());
            } else {
                let float = number.as_f64().unwrap_or(f64::NAN);
                // A float is kept only if its decimal-model spelling reads back to the same
                // number: refused rather than silently altered.
                let spelled = format!("{float:.places$}", places = DECIMAL_PLACES as usize);
                let round_trips = spelled
                    .trim_end_matches('0')
                    .trim_end_matches('.')
                    .parse::<f64>()
                    .is_ok_and(|read| read.to_bits() == float.to_bits());
                if !float.is_finite() || !round_trips || float.abs() > MAX_DECIMAL {
                    return Err(refuse(
                        "invalid_json",
                        pointer,
                        format!(
                            "{label}: a number is an integer or a decimal to {DECIMAL_PLACES} places below {MAX_DECIMAL}"
                        ),
                    ));
                }
                write_number(out, float, float.fract() == 0.0);
            }
        }
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_json(item, out, depth + 1, pointer, label)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort();
            for (index, key) in keys.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, key);
                out.push(':');
                write_json(&map[*key], out, depth + 1, pointer, label)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

/// The string kinds checked by a shape: each names its check, refusal code and hint.
fn formatted(
    kind: &FieldKind,
    value: &Value,
    pointer: &Pointer,
    label: &str,
    out: &mut String,
) -> Result<(), Refusal> {
    let (check, code, hint): (fn(&str) -> bool, &str, &str) = match kind {
        FieldKind::DateTime => (
            is_utc_timestamp,
            "invalid_datetime",
            "a UTC time like 2026-10-25T14:30:00Z",
        ),
        FieldKind::Date => (is_date, "invalid_date", "a day like 2026-10-25"),
        FieldKind::Time => (is_time, "invalid_time", "a time of day like 14:30"),
        FieldKind::Email => (is_email, "invalid_email", "an email address"),
        FieldKind::Url => (
            is_url,
            "invalid_url",
            "a web address starting http:// or https://",
        ),
        _ => (is_color, "invalid_color", "a colour like #1a6fdd"),
    };
    let text = value
        .as_str()
        .filter(|text| check(text))
        .ok_or_else(|| refuse(code, pointer, format!("{label} is {hint}")))?;
    write_string(out, text);
    Ok(())
}

/// A `json` field's value: canonicalised, bounded in bytes, written to `out`.
fn json_value(
    value: &Value,
    max_bytes: u64,
    pointer: &Pointer,
    label: &str,
    out: &mut String,
) -> Result<(), Refusal> {
    let mut canonical = String::new();
    write_json(value, &mut canonical, 0, pointer, label)?;
    if u64::try_from(canonical.len()).unwrap_or(u64::MAX) > max_bytes {
        return Err(refuse(
            "too_large",
            pointer,
            format!("{label} is at most {max_bytes} bytes"),
        ));
    }
    out.push_str(&canonical);
    Ok(())
}

/// A markdown field's limit: `max_chars` (default [`MAX_TEXT_CHARS`]).
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

/// A JSON field's limit: `max_bytes` (default [`MAX_JSON_BYTES`]).
fn json_kind(object: &Map<String, Value>, pointer: &Pointer) -> Result<FieldKind, Refusal> {
    let max_bytes = match object.get("max_bytes") {
        None => MAX_JSON_BYTES,
        Some(value) => value
            .as_u64()
            .filter(|max| (1..=MAX_JSON_BYTES).contains(max))
            .ok_or_else(|| {
                refuse(
                    "out_of_range",
                    &at(pointer, "max_bytes"),
                    format!("`max_bytes` is a whole number from 1 to {MAX_JSON_BYTES}"),
                )
            })?,
    };
    Ok(FieldKind::Json { max_bytes })
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
        | (FieldKind::Date, FieldKind::Date)
        | (FieldKind::Time, FieldKind::Time)
        | (FieldKind::Email, FieldKind::Email)
        | (FieldKind::Url, FieldKind::Url)
        | (FieldKind::Color, FieldKind::Color) => true,
        // Any text value is Markdown if it fits the new limit; Markdown is text only for a
        // multi-line target that fits (stored values may hold line breaks).
        (
            FieldKind::Text {
                max_chars: old_max, ..
            }
            | FieldKind::Markdown { max_chars: old_max },
            FieldKind::Markdown { max_chars: new_max },
        )
        | (
            FieldKind::Markdown { max_chars: old_max },
            FieldKind::Text {
                max_chars: new_max,
                multiline: true,
            },
        ) => new_max >= old_max,
        (FieldKind::Json { max_bytes: old_max }, FieldKind::Json { max_bytes: new_max }) => {
            new_max >= old_max
        }
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
                    default: None,
                    when: None,
                    unique: false,
                },
                Field {
                    id: "body".to_owned(),
                    label: "Body".to_owned(),
                    required: true,
                    kind: FieldKind::RichText,
                    default: None,
                    when: None,
                    unique: false,
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
        Self::check_whens(&fields, &root)?;
        Ok(Self {
            name,
            title_field,
            fields,
        })
    }

    /// A `when` can only name a field that exists, and never the field itself.
    fn check_whens(fields: &[Field], root: &Pointer) -> Result<(), Refusal> {
        for (index, field) in fields.iter().enumerate() {
            if let Some(clause) = &field.when {
                let mut pointer = at(root, "fields");
                pointer.push_index(index);
                pointer.push("when");
                if clause.field == field.id {
                    return Err(refuse(
                        "invalid_when",
                        &at(&pointer, "field"),
                        "a field can't depend on itself",
                    ));
                }
                if !fields.iter().any(|other| other.id == clause.field) {
                    return Err(refuse(
                        "unknown_field",
                        &at(&pointer, "field"),
                        format!("no field named `{}`", clause.field),
                    ));
                }
            }
        }
        Ok(())
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
        let common = [
            "id", "label", "type", "required", "default", "when", "unique",
        ];
        let kind = match object.get("type").and_then(Value::as_str) {
            Some("text") => {
                only(
                    object,
                    &[&common[..], &["max_chars", "multiline"]].concat(),
                    pointer,
                )?;
                text_kind(object, pointer)?
            }
            Some("number") => {
                only(
                    object,
                    &[&common[..], &["integer", "min", "max"]].concat(),
                    pointer,
                )?;
                number_kind(object, pointer)?
            }
            Some("enum") => {
                only(object, &[&common[..], &["values"]].concat(), pointer)?;
                FieldKind::Enum {
                    values: enum_values(object, pointer)?,
                }
            }
            Some("markdown") => {
                only(object, &[&common[..], &["max_chars"]].concat(), pointer)?;
                markdown_kind(object, pointer)?
            }
            Some("json") => {
                only(object, &[&common[..], &["max_bytes"]].concat(), pointer)?;
                json_kind(object, pointer)?
            }
            other => {
                let kind = match other {
                    Some("rich_text") => FieldKind::RichText,
                    Some("boolean") => FieldKind::Boolean,
                    Some("datetime") => FieldKind::DateTime,
                    Some("date") => FieldKind::Date,
                    Some("time") => FieldKind::Time,
                    Some("email") => FieldKind::Email,
                    Some("url") => FieldKind::Url,
                    Some("color") => FieldKind::Color,
                    _ => {
                        return Err(refuse(
                            "invalid_type",
                            &at(pointer, "type"),
                            "`type` is one of text, rich_text, number, boolean, enum, datetime, date, time, email, url, markdown, color",
                        ));
                    }
                };
                only(object, &common, pointer)?;
                kind
            }
        };
        let mut field = Field {
            id,
            label,
            required,
            kind,
            default: None,
            when: None,
            unique: false,
        };
        Self::field_options(&mut field, object, pointer)?;
        Ok(field)
    }

    /// The options any kind can carry: `default` (checked like a value, kept canonical) and
    /// `when` (display-only, refused on a required field).
    fn field_options(
        field: &mut Field,
        object: &Map<String, Value>,
        pointer: &Pointer,
    ) -> Result<(), Refusal> {
        if let Some(default) = object.get("default") {
            // The default must hold for the kind: it goes through the same check a value does
            // and keeps the canonical spelling, so writing it later changes no bytes.
            let mut canonical = String::new();
            Self::value(field, default, &at(pointer, "default"), &mut canonical)?;
            field.default = Some(canonical);
        }
        if let Some(clause) = object.get("when") {
            field.when = Some(Self::when_clause(clause, &at(pointer, "when"))?);
            if field.required {
                return Err(refuse(
                    "invalid_when",
                    &at(pointer, "when"),
                    "a required field can't hide",
                ));
            }
        }
        field.unique = bool_or(object, "unique", false, pointer)?;
        if field.unique
            && matches!(
                field.kind,
                FieldKind::RichText
                    | FieldKind::Markdown { .. }
                    | FieldKind::Json { .. }
                    | FieldKind::Text {
                        multiline: true,
                        ..
                    }
            )
        {
            return Err(refuse(
                "invalid_unique",
                &at(pointer, "unique"),
                "a unique value must be a single, short value",
            ));
        }
        Ok(())
    }

    /// A `when` clause: `{"field": <id>, "in": [<string>, ...]}`, 1 to 10 distinct plain
    /// strings. Whether the named field exists is checked once every field is parsed.
    fn when_clause(value: &Value, pointer: &Pointer) -> Result<When, Refusal> {
        let object = value.as_object().ok_or_else(|| {
            refuse(
                "invalid_when",
                pointer,
                "`when` is an object like {\"field\": \"section\", \"in\": [\"News\"]}",
            )
        })?;
        only(object, &["field", "in"], pointer)?;
        let field = object
            .get("field")
            .and_then(Value::as_str)
            .filter(|field| is_field_id(field))
            .ok_or_else(|| {
                refuse(
                    "invalid_when",
                    &at(pointer, "field"),
                    "`when.field` is another field's id",
                )
            })?
            .to_owned();
        let options = object
            .get("in")
            .and_then(Value::as_array)
            .filter(|values| !values.is_empty() && values.len() <= 10)
            .ok_or_else(|| {
                refuse(
                    "invalid_when",
                    &at(pointer, "in"),
                    "`when.in` is 1 to 10 strings",
                )
            })?
            .iter()
            .map(|value| {
                value
                    .as_str()
                    .filter(|option| plain_text(option, 100))
                    .map(str::to_owned)
                    .ok_or_else(|| {
                        refuse(
                            "invalid_when",
                            &at(pointer, "in"),
                            "`when.in` is 1 to 10 strings",
                        )
                    })
            })
            .collect::<Result<Vec<String>, Refusal>>()?;
        if options
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len()
            != options.len()
        {
            return Err(refuse(
                "invalid_when",
                &at(pointer, "in"),
                "`when.in` repeats a value",
            ));
        }
        Ok(When { field, options })
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
                FieldKind::Markdown { max_chars } => {
                    out.push_str(r#","max_chars":"#);
                    out.push_str(&max_chars.to_string());
                }
                FieldKind::Json { max_bytes } => {
                    out.push_str(r#","max_bytes":"#);
                    out.push_str(&max_bytes.to_string());
                }
                FieldKind::RichText
                | FieldKind::Boolean
                | FieldKind::DateTime
                | FieldKind::Date
                | FieldKind::Time
                | FieldKind::Email
                | FieldKind::Url
                | FieldKind::Color => {}
            }
            if let Some(default) = &field.default {
                out.push_str(r#","default":"#);
                out.push_str(default);
            }
            if let Some(clause) = &field.when {
                out.push_str(r#","when":{"field":"#);
                write_string(&mut out, &clause.field);
                out.push_str(r#","in":["#);
                for (index, option) in clause.options.iter().enumerate() {
                    if index > 0 {
                        out.push(',');
                    }
                    write_string(&mut out, option);
                }
                out.push_str("]}");
            }
            if field.unique {
                out.push_str(r#","unique":true"#);
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
        let mut values: Vec<(String, String)> = Vec::with_capacity(self.fields.len());
        out.push('{');
        let mut first = true;
        for field in &self.fields {
            let pointer = at(&root, &field.id);
            if object.get(&field.id).is_none()
                && let Some(canonical) = &field.default
            {
                // Left out with a default: the entry gets it, already canonical.
                if !first {
                    out.push(',');
                }
                first = false;
                write_string(&mut out, &field.id);
                out.push(':');
                let start = out.len();
                out.push_str(canonical);
                values.push((field.id.clone(), out[start..].to_owned()));
                if field.id == self.title_field
                    && let Some(text) = serde_json::from_str::<Value>(canonical)
                        .ok()
                        .and_then(|parsed| parsed.as_str().map(str::to_owned))
                {
                    title = text;
                }
                continue;
            }
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
            let start = out.len();
            Self::value(field, value, &pointer, &mut out)?;
            values.push((field.id.clone(), out[start..].to_owned()));
            if field.id == self.title_field {
                value.as_str().unwrap_or_default().clone_into(&mut title);
            }
        }
        out.push('}');
        Ok(Content {
            canonical: out,
            title,
            values,
        })
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
            FieldKind::DateTime
            | FieldKind::Date
            | FieldKind::Time
            | FieldKind::Email
            | FieldKind::Url
            | FieldKind::Color => formatted(&field.kind, value, pointer, label, out)?,
            FieldKind::Markdown { max_chars } => {
                let text = value
                    .as_str()
                    .ok_or_else(|| refuse("wrong_type", pointer, format!("{label} is text")))?;
                check_text(text, label, *max_chars, true, pointer)?;
                write_string(out, text);
            }
            FieldKind::Json { max_bytes } => {
                json_value(value, *max_bytes, pointer, label, out)?;
            }
        }
        Ok(())
    }

    /// Checks that `next` can replace this definition without a migration: every stored entry
    /// stays valid. Allowed: a new name or labels, new optional fields, wider limits, more enum
    /// options, an integer field becoming a decimal one, a single-line text becoming multi-line.
    pub fn check_successor(&self, next: &Self) -> Result<(), Refusal> {
        let root = Pointer::new();
        if next.title_field != self.title_field {
            return Err(refuse(
                "needs_migration",
                &at(&root, "title_field"),
                "changing the title field needs a migration",
            ));
        }
        for old in &self.fields {
            let Some((index, new)) = next
                .fields
                .iter()
                .enumerate()
                .find(|(_, field)| field.id == old.id)
            else {
                return Err(refuse(
                    "needs_migration",
                    &at(&root, "fields"),
                    format!("removing `{}` needs a migration", old.id),
                ));
            };
            let mut pointer = at(&root, "fields");
            pointer.push_index(index);
            let wider = widens(&old.kind, &new.kind);
            if !wider || (new.required && !old.required) || (new.unique && !old.unique) {
                return Err(refuse(
                    "needs_migration",
                    &pointer,
                    format!(
                        "this change to `{}` could make stored content invalid; it needs a migration",
                        old.id
                    ),
                ));
            }
        }
        for (index, new) in next.fields.iter().enumerate() {
            if new.required && !self.fields.iter().any(|old| old.id == new.id) {
                let mut pointer = at(&root, "fields");
                pointer.push_index(index);
                return Err(refuse(
                    "needs_migration",
                    &pointer,
                    format!(
                        "a new field (`{}`) must be optional: existing entries don't have it",
                        new.id
                    ),
                ));
            }
        }
        Ok(())
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
        assert_eq!(base.check_successor(&wider), Ok(()));

        let change = |edit: &dyn Fn(&mut Value)| {
            let mut definition: Value = serde_json::from_str(&base.canonical_json()).expect("JSON");
            edit(&mut definition);
            base.check_successor(&ContentType::parse(&definition).expect("valid"))
                .map_err(|refusal| refusal.code)
        };
        let risky: [&dyn Fn(&mut Value); 6] = [
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
        ];
        for edit in risky {
            assert_eq!(change(edit), Err("needs_migration"));
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

    #[test]
    fn date_time_email_and_url_fields_parse_and_check_their_values() {
        let kind = ContentType::parse(&json!({
            "name": "Contacts",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "day", "label": "Day", "type": "date"},
                {"id": "opens", "label": "Opens", "type": "time"},
                {"id": "address", "label": "Address", "type": "email"},
                {"id": "site", "label": "Site", "type": "url"}
            ]
        }))
        .expect("valid definition");
        let canonical = kind.canonical_json();
        for kind in ["date", "time", "email", "url"] {
            assert!(
                canonical.contains(&format!(r#""type":"{kind}""#)),
                "{canonical}"
            );
        }
        for (field, good) in [
            ("day", "2026-02-28"),
            ("day", "2028-02-29"),
            ("opens", "14:30"),
            ("opens", "09:15:42"),
            ("address", "editor@colophon.dev"),
            ("site", "https://example.com/a?q=1#frag"),
            ("site", "http://localhost:8080"),
        ] {
            kind.validate(&json!({"name": "A", field: good}))
                .unwrap_or_else(|refusal| panic!("{field} = {good}: {}", refusal.message));
        }
        for (field, bad, code) in [
            ("day", "2026-02-29", "invalid_date"),
            ("day", "2026-2-8", "invalid_date"),
            ("opens", "24:00", "invalid_time"),
            ("opens", "9:15", "invalid_time"),
            ("opens", "14:60", "invalid_time"),
            ("address", "not-an-email", "invalid_email"),
            ("address", "a@@b.com", "invalid_email"),
            ("address", "a@localhost", "invalid_email"),
            ("address", "a@.com", "invalid_email"),
            ("site", "javascript:alert(1)", "invalid_url"),
            ("site", "https://u:p@h.com", "invalid_url"),
            ("site", "https://", "invalid_url"),
            ("site", "ftp://h", "invalid_url"),
        ] {
            let refusal = kind
                .validate(&json!({"name": "A", field: bad}))
                .expect_err(&format!("{field} = {bad}"));
            assert_eq!(refusal.code, code, "{field} = {bad}");
            assert_eq!(refusal.pointer, format!("/{field}"));
        }
    }

    #[test]
    fn markdown_and_color_fields_parse_and_check_their_values() {
        let kind = ContentType::parse(&json!({
            "name": "Notes",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "body", "label": "Body", "type": "markdown", "max_chars": 20},
                {"id": "tint", "label": "Tint", "type": "color"}
            ]
        }))
        .expect("valid definition");
        let canonical = kind.canonical_json();
        assert!(
            canonical.contains(r#""type":"markdown","required":false,"max_chars":20"#),
            "{canonical}"
        );
        assert!(
            canonical.contains(r#""type":"color","required":false}"#),
            "{canonical}"
        );
        let defaulted = ContentType::parse(&json!({
            "name": "Notes",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "body", "label": "Body", "type": "markdown"}
            ]
        }))
        .expect("valid definition");
        assert!(defaulted.canonical_json().contains(&format!(
            r#""type":"markdown","required":false,"max_chars":{MAX_TEXT_CHARS}"#
        )));
        for (field, good) in [
            ("body", "# Title\n**b**"),
            ("tint", "#1a6fdd"),
            ("tint", "#000000"),
        ] {
            kind.validate(&json!({"name": "A", field: good}))
                .unwrap_or_else(|refusal| panic!("{field} = {good}: {}", refusal.message));
        }
        for (field, bad, code) in [
            ("body", "this is longer than twenty characters", "too_long"),
            ("body", "", "invalid_text"),
            ("tint", "#AABBCC", "invalid_color"),
            ("tint", "red", "invalid_color"),
            ("tint", "#1a6fdd ", "invalid_color"),
            ("tint", "#1a6fd", "invalid_color"),
        ] {
            let refusal = kind
                .validate(&json!({"name": "A", field: bad}))
                .expect_err(&format!("{field} = {bad}"));
            assert_eq!(refusal.code, code, "{field} = {bad}");
            assert_eq!(refusal.pointer, format!("/{field}"));
        }
    }

    #[test]
    fn json_fields_parse_bound_values_and_write_a_sorted_canonical_form() {
        let kind = ContentType::parse(&json!({
            "name": "Blocks",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "data", "label": "Data", "type": "json", "max_bytes": 40}
            ]
        }))
        .expect("valid definition");
        let canonical = kind.canonical_json();
        assert!(
            canonical.contains(r#""type":"json","required":false,"max_bytes":40"#),
            "{canonical}"
        );
        // Keys are sorted; scalars are values too; numbers keep the decimal model's spelling.
        let content = kind
            .validate(&json!({"name": "A", "data": {"b": [1, 2.5, null, true], "a": "x"}}))
            .expect("valid");
        assert_eq!(
            content.canonical_json(),
            r#"{"name":"A","data":{"a":"x","b":[1,2.5,null,true]}}"#
        );
        for good in [
            json!("a string"),
            json!(7),
            json!(-4),
            json!(18_446_744_073_709_551_615_u64),
            json!(4.5),
            json!(true),
            json!(null),
            json!([1, [2, [3]]]),
        ] {
            kind.validate(&json!({"name": "A", "data": good}))
                .unwrap_or_else(|refusal| panic!("{good}: {}", refusal.message));
        }
        let mut deep = json!(0);
        for _ in 0..40 {
            deep = json!([deep]);
        }
        for (bad, code) in [
            (deep, "too_deep"),
            (json!({"a": "x".repeat(60)}), "too_large"),
            (json!(1.234_567_89), "invalid_json"),
            (json!(2_000_000_000.5), "invalid_json"),
            (json!(1e-7), "invalid_json"),
        ] {
            let refusal = kind
                .validate(&json!({"name": "A", "data": bad}))
                .expect_err("must be refused");
            assert_eq!(refusal.code, code, "{bad}");
            assert_eq!(refusal.pointer, "/data");
        }
    }

    #[test]
    fn a_default_is_checked_at_parse_fills_what_is_left_out_and_keeps_its_canonical_spelling() {
        let kind = ContentType::parse(&json!({
            "name": "Posts",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"], "default": "Guides"},
                {"id": "featured", "label": "Featured", "type": "boolean", "default": false},
                {"id": "rating", "label": "Rating", "type": "number", "default": 3}
            ]
        }))
        .expect("valid definition");
        let canonical = kind.canonical_json();
        for spelling in [
            r#""type":"enum","required":false,"values":["News","Guides"],"default":"Guides""#,
            r#""type":"boolean","required":false,"default":false"#,
            r#""type":"number","required":false,"integer":false,"default":3"#,
        ] {
            assert!(canonical.contains(spelling), "{canonical}");
        }
        // Left out: the entry gets the defaults, in definition order.
        let content = kind
            .validate(&json!({"headline": "Hi"}))
            .expect("defaults fill what is left out");
        assert_eq!(
            content.canonical_json(),
            r#"{"headline":"Hi","section":"Guides","featured":false,"rating":3}"#
        );
        // Given: the entry's value wins.
        let content = kind
            .validate(&json!({"headline": "Hi", "section": "News", "rating": 5}))
            .expect("valid");
        assert_eq!(
            content.canonical_json(),
            r#"{"headline":"Hi","section":"News","featured":false,"rating":5}"#
        );
        // A default the kind refuses is refused at parse, pointing at it.
        for (default, code) in [
            (json!("Sports"), "not_an_option"),
            (json!(true), "not_an_option"),
        ] {
            let refusal = ContentType::parse(&json!({
                "name": "Posts",
                "title_field": "headline",
                "fields": [
                    {"id": "headline", "label": "Headline", "type": "text", "required": true},
                    {"id": "section", "label": "Section", "type": "enum", "values": ["News"], "default": default}
                ]
            }))
            .expect_err("must be refused");
            assert_eq!(refusal.code, code, "{default}");
            assert_eq!(refusal.pointer, "/fields/1/default");
        }
    }

    #[test]
    fn a_when_clause_names_another_field_and_never_hides_a_required_one() {
        let kind = ContentType::parse(&json!({
            "name": "Posts",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"]},
                {"id": "sub", "label": "Sub-section", "type": "text", "when": {"field": "section", "in": ["Guides"]}}
            ]
        }))
        .expect("valid definition");
        assert!(
            kind.canonical_json()
                .contains(r#""when":{"field":"section","in":["Guides"]}"#),
            "{}",
            kind.canonical_json()
        );
        for (clause, code) in [
            (json!({"field": "nowhere", "in": ["x"]}), "unknown_field"),
            (json!({"field": "sub", "in": ["x"]}), "invalid_when"),
            (json!({"field": "section", "in": []}), "invalid_when"),
            (
                json!({"field": "section", "in": ["a", "a"]}),
                "invalid_when",
            ),
        ] {
            let refusal = ContentType::parse(&json!({
                "name": "Posts",
                "title_field": "headline",
                "fields": [
                    {"id": "headline", "label": "Headline", "type": "text", "required": true},
                    {"id": "section", "label": "Section", "type": "enum", "values": ["News"]},
                    {"id": "sub", "label": "Sub", "type": "text", "when": clause}
                ]
            }))
            .expect_err("must be refused");
            assert_eq!(refusal.code, code, "{clause}");
        }
        let refusal = ContentType::parse(&json!({
            "name": "Posts",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News"]},
                {"id": "sub", "label": "Sub", "type": "text", "required": true, "when": {"field": "section", "in": ["News"]}}
            ]
        }))
        .expect_err("a required field can't hide");
        assert_eq!(refusal.code, "invalid_when");
    }

    #[test]
    fn a_unique_field_is_kept_to_single_short_values_and_written_true() {
        let kind = ContentType::parse(&json!({
            "name": "Authors",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "email", "label": "Email", "type": "email", "unique": true}
            ]
        }))
        .expect("valid definition");
        assert!(
            kind.canonical_json()
                .contains(r#""type":"email","required":false,"unique":true"#),
            "{}",
            kind.canonical_json()
        );
        for field in [
            json!({"id": "m", "label": "M", "type": "markdown", "unique": true}),
            json!({"id": "m", "label": "M", "type": "rich_text", "unique": true}),
            json!({"id": "m", "label": "M", "type": "json", "unique": true}),
            json!({"id": "m", "label": "M", "type": "text", "multiline": true, "unique": true}),
        ] {
            let refusal = ContentType::parse(&json!({
                "name": "Authors",
                "title_field": "name",
                "fields": [
                    {"id": "name", "label": "Name", "type": "text", "required": true},
                    field
                ]
            }))
            .expect_err("must be refused");
            assert_eq!(refusal.code, "invalid_unique");
            assert_eq!(refusal.pointer, "/fields/1/unique");
        }
        let before = ContentType::parse(&json!({
            "name": "Authors",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true},
                {"id": "email", "label": "Email", "type": "email"}
            ]
        }))
        .expect("valid");
        let refusal = before
            .check_successor(&kind)
            .expect_err("making a stored field unique can't be checked without the content");
        assert_eq!(refusal.code, "needs_migration");
        kind.check_successor(&before)
            .expect("dropping unique is always safe");
    }

    #[test]
    fn text_widens_into_markdown_that_fits_and_markdown_only_into_wide_multi_line_text() {
        let first = |fields: Value| {
            ContentType::parse(&json!({
                "name": "Notes",
                "title_field": "n",
                "fields": fields
            }))
            .expect("valid definition")
        };
        let n = json!({"id": "n", "label": "N", "type": "text", "required": true});
        let with = |field: Value| first(json!([n.clone(), field]));
        let old = with(json!({"id": "b", "label": "B", "type": "text", "max_chars": 200}));
        old.check_successor(&with(
            json!({"id": "b", "label": "B", "type": "markdown", "max_chars": 500}),
        ))
        .expect("text widens to markdown");
        old.check_successor(&with(
            json!({"id": "b", "label": "B", "type": "markdown", "max_chars": 100}),
        ))
        .expect_err("shorter markdown does not widen");
        let md = with(json!({"id": "b", "label": "B", "type": "markdown", "max_chars": 500}));
        md.check_successor(&with(
            json!({"id": "b", "label": "B", "type": "text", "max_chars": 500, "multiline": true}),
        ))
        .expect("markdown widens to multi-line text");
        md.check_successor(&with(
            json!({"id": "b", "label": "B", "type": "text", "max_chars": 500}),
        ))
        .expect_err("single-line text would reject stored line breaks");
        md.check_successor(&with(json!({"id": "b", "label": "B", "type": "color"})))
            .expect_err("a kind change needs a migration");
    }
}
