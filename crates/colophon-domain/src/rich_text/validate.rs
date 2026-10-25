//! The validator: one pass over the JSON value that checks every rule, stops at the first
//! problem in the order the format document defines, and writes the canonical form as it goes.
//!
//! Recursion is bounded: a node deeper than `MAX_DEPTH` is refused before its children are
//! looked at, so hostile nesting can't exhaust the stack.

use serde_json::{Map, Value};

use super::{
    MAX_ALT_CHARS, MAX_BYTES, MAX_DEPTH, MAX_NODES, RejectCode, Rejection, RichText, SCHEMA,
    VERSION, href::is_allowed_href, json::write_string, pointer::Pointer,
};

/// Checks `value` against `colophon/rich-text` v1 and returns it in canonical form.
pub fn validate(value: &Value) -> Result<RichText, Rejection> {
    let mut validator = Validator {
        pointer: Pointer::new(),
        out: String::with_capacity(256),
        nodes: 0,
        depth: 0,
    };
    validator.envelope(value)?;
    if validator.out.len() > MAX_BYTES {
        return Err(Rejection {
            code: RejectCode::TooLarge,
            pointer: String::new(),
        });
    }
    Ok(RichText {
        canonical: validator.out,
        nodes: validator.nodes,
        depth: validator.depth,
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Doc,
    Paragraph,
    Heading,
    BulletList,
    OrderedList,
    ListItem,
    Blockquote,
    CodeBlock,
    HorizontalRule,
    Image,
    Callout,
    Text,
    HardBreak,
}

/// What a node may contain.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Model {
    /// No content (a leaf).
    Leaf,
    /// One or more blocks.
    Blocks,
    /// Zero or more inline nodes.
    Inline,
    /// One or more list items.
    ListItems,
    /// A paragraph, then zero or more blocks.
    ListItem,
    /// One or more paragraphs.
    Paragraphs,
    /// Zero or more text nodes, without marks.
    CodeText,
}

impl Kind {
    fn parse(name: &str) -> Option<Self> {
        Some(match name {
            "doc" => Self::Doc,
            "paragraph" => Self::Paragraph,
            "heading" => Self::Heading,
            "bulletList" => Self::BulletList,
            "orderedList" => Self::OrderedList,
            "listItem" => Self::ListItem,
            "blockquote" => Self::Blockquote,
            "codeBlock" => Self::CodeBlock,
            "horizontalRule" => Self::HorizontalRule,
            "image" => Self::Image,
            "callout" => Self::Callout,
            "text" => Self::Text,
            "hardBreak" => Self::HardBreak,
            _ => return None,
        })
    }

    const fn name(self) -> &'static str {
        match self {
            Self::Doc => "doc",
            Self::Paragraph => "paragraph",
            Self::Heading => "heading",
            Self::BulletList => "bulletList",
            Self::OrderedList => "orderedList",
            Self::ListItem => "listItem",
            Self::Blockquote => "blockquote",
            Self::CodeBlock => "codeBlock",
            Self::HorizontalRule => "horizontalRule",
            Self::Image => "image",
            Self::Callout => "callout",
            Self::Text => "text",
            Self::HardBreak => "hardBreak",
        }
    }

    const fn is_block(self) -> bool {
        matches!(
            self,
            Self::Paragraph
                | Self::Heading
                | Self::BulletList
                | Self::OrderedList
                | Self::Blockquote
                | Self::CodeBlock
                | Self::HorizontalRule
                | Self::Image
                | Self::Callout
        )
    }

    const fn model(self) -> Model {
        match self {
            Self::Doc | Self::Blockquote => Model::Blocks,
            Self::Paragraph | Self::Heading => Model::Inline,
            Self::BulletList | Self::OrderedList => Model::ListItems,
            Self::ListItem => Model::ListItem,
            Self::Callout => Model::Paragraphs,
            Self::CodeBlock => Model::CodeText,
            Self::HorizontalRule | Self::Image | Self::Text | Self::HardBreak => Model::Leaf,
        }
    }

    /// The fields a node object may have.
    const fn fields(self) -> &'static [&'static str] {
        match self {
            Self::Doc | Self::Paragraph | Self::BulletList | Self::ListItem | Self::Blockquote => {
                &["type", "content"]
            }
            Self::Heading | Self::OrderedList | Self::CodeBlock | Self::Callout => {
                &["type", "attrs", "content"]
            }
            Self::Image => &["type", "attrs"],
            Self::Text => &["type", "text", "marks"],
            Self::HorizontalRule | Self::HardBreak => &["type"],
        }
    }
}

impl Model {
    /// Whether `child` may appear at `index`.
    fn allows(self, index: usize, child: Kind) -> bool {
        match self {
            Self::Leaf => false,
            Self::Blocks => child.is_block(),
            Self::Inline => matches!(child, Kind::Text | Kind::HardBreak),
            Self::ListItems => child == Kind::ListItem,
            Self::ListItem => {
                if index == 0 {
                    child == Kind::Paragraph
                } else {
                    child.is_block()
                }
            }
            Self::Paragraphs => child == Kind::Paragraph,
            Self::CodeText => child == Kind::Text,
        }
    }

    const fn requires_content(self) -> bool {
        matches!(
            self,
            Self::Blocks | Self::ListItems | Self::ListItem | Self::Paragraphs
        )
    }
}

const MARK_ORDER: [&str; 5] = ["link", "bold", "code", "italic", "strike"];

struct Validator {
    pointer: Pointer,
    out: String,
    nodes: usize,
    depth: usize,
}

impl Validator {
    fn reject(&self, code: RejectCode) -> Rejection {
        Rejection {
            code,
            pointer: self.pointer.as_str().to_owned(),
        }
    }

    fn reject_at(&self, code: RejectCode, field: &str) -> Rejection {
        Rejection {
            code,
            pointer: self.pointer.with(field),
        }
    }

    fn envelope(&mut self, value: &Value) -> Result<(), Rejection> {
        let Value::Object(envelope) = value else {
            return Err(self.reject(RejectCode::WrongType));
        };
        match envelope.get("schema") {
            None => return Err(self.reject_at(RejectCode::MissingField, "schema")),
            Some(Value::String(schema)) if schema == SCHEMA => {}
            Some(Value::String(_)) => {
                return Err(self.reject_at(RejectCode::UnsupportedSchema, "schema"));
            }
            Some(_) => return Err(self.reject_at(RejectCode::WrongType, "schema")),
        }
        match envelope.get("version") {
            None => return Err(self.reject_at(RejectCode::MissingField, "version")),
            Some(Value::Number(version)) if version.as_u64() == Some(VERSION) => {}
            Some(Value::Number(_)) => {
                return Err(self.reject_at(RejectCode::UnsupportedVersion, "version"));
            }
            Some(_) => return Err(self.reject_at(RejectCode::WrongType, "version")),
        }
        self.unknown_fields(envelope, &["schema", "version", "doc"])?;
        let Some(doc) = envelope.get("doc") else {
            return Err(self.reject_at(RejectCode::MissingField, "doc"));
        };

        self.out
            .push_str(r#"{"schema":"colophon/rich-text","version":1,"doc":"#);
        self.pointer.push("doc");
        let kind = self.node_kind(doc, 1)?;
        if kind != Kind::Doc {
            return Err(self.reject(RejectCode::InvalidContent));
        }
        self.node_body(kind, doc, 1, false)?;
        self.pointer.pop();
        self.out.push('}');
        Ok(())
    }

    /// Reports the first field (in sorted order) that isn't in `allowed`.
    fn unknown_fields(
        &self,
        object: &Map<String, Value>,
        allowed: &[&str],
    ) -> Result<(), Rejection> {
        let mut unknown: Vec<&String> = object
            .keys()
            .filter(|key| !allowed.contains(&key.as_str()))
            .collect();
        unknown.sort_unstable();
        match unknown.first() {
            Some(key) => Err(self.reject_at(RejectCode::UnknownField, key)),
            None => Ok(()),
        }
    }

    /// Counts the node, checks its depth, and reads its type. The pointer is at the node.
    fn node_kind(&mut self, value: &Value, depth: usize) -> Result<Kind, Rejection> {
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return Err(Rejection {
                code: RejectCode::TooManyNodes,
                pointer: String::new(),
            });
        }
        let Value::Object(node) = value else {
            return Err(self.reject(RejectCode::WrongType));
        };
        if depth > MAX_DEPTH {
            return Err(self.reject(RejectCode::TooDeep));
        }
        self.depth = self.depth.max(depth);
        match node.get("type") {
            None => Err(self.reject_at(RejectCode::MissingField, "type")),
            Some(Value::String(name)) => {
                Kind::parse(name).ok_or_else(|| self.reject_at(RejectCode::UnknownNode, "type"))
            }
            Some(_) => Err(self.reject_at(RejectCode::WrongType, "type")),
        }
    }

    /// Everything after the type and position checks; writes the node's canonical form.
    fn node_body(
        &mut self,
        kind: Kind,
        value: &Value,
        depth: usize,
        in_code: bool,
    ) -> Result<(), Rejection> {
        let Value::Object(node) = value else {
            return Err(self.reject(RejectCode::WrongType));
        };
        self.unknown_fields(node, kind.fields())?;

        self.out.push_str(r#"{"type":"#);
        write_string(&mut self.out, kind.name());
        self.attrs(kind, node)?;
        if kind == Kind::Text {
            self.text(node, in_code)?;
        }
        self.content(kind, node, depth)?;
        self.out.push('}');
        Ok(())
    }

    fn attrs(&mut self, kind: Kind, node: &Map<String, Value>) -> Result<(), Rejection> {
        let required = matches!(kind, Kind::Heading | Kind::Image | Kind::Callout);
        let attrs = match node.get("attrs") {
            None if required => return Err(self.reject_at(RejectCode::MissingField, "attrs")),
            None => return Ok(()),
            Some(Value::Object(attrs)) => attrs,
            Some(_) => return Err(self.reject_at(RejectCode::WrongType, "attrs")),
        };
        self.pointer.push("attrs");
        let result = self.attrs_of(kind, attrs);
        self.pointer.pop();
        result
    }

    fn attrs_of(&mut self, kind: Kind, attrs: &Map<String, Value>) -> Result<(), Rejection> {
        match kind {
            Kind::Heading => {
                self.unknown_fields(attrs, &["level"])?;
                let level = self.integer(attrs, "level", true, 2..=4)?;
                self.out.push_str(r#","attrs":{"level":"#);
                self.out.push_str(&level.unwrap_or_default().to_string());
                self.out.push('}');
            }
            Kind::OrderedList => {
                self.unknown_fields(attrs, &["start"])?;
                if let Some(start) = self.integer(attrs, "start", false, 2..=100_000)? {
                    self.out.push_str(r#","attrs":{"start":"#);
                    self.out.push_str(&start.to_string());
                    self.out.push('}');
                }
            }
            Kind::CodeBlock => {
                self.unknown_fields(attrs, &["language"])?;
                if let Some(language) = self.string(attrs, "language", false)? {
                    if !is_code_language(language) {
                        return Err(self.reject_at(RejectCode::InvalidAttribute, "language"));
                    }
                    self.out.push_str(r#","attrs":{"language":"#);
                    write_string(&mut self.out, language);
                    self.out.push('}');
                }
            }
            Kind::Image => {
                self.unknown_fields(attrs, &["assetId", "alt"])?;
                let asset_id = self.string(attrs, "assetId", true)?.unwrap_or_default();
                if !is_lowercase_uuid(asset_id) {
                    return Err(self.reject_at(RejectCode::InvalidAttribute, "assetId"));
                }
                let alt = self.string(attrs, "alt", true)?.unwrap_or_default();
                if alt.chars().all(is_blank) || alt.chars().count() > MAX_ALT_CHARS {
                    return Err(self.reject_at(RejectCode::InvalidAttribute, "alt"));
                }
                self.out.push_str(r#","attrs":{"assetId":"#);
                write_string(&mut self.out, asset_id);
                self.out.push_str(r#","alt":"#);
                write_string(&mut self.out, alt);
                self.out.push('}');
            }
            Kind::Callout => {
                self.unknown_fields(attrs, &["tone"])?;
                let tone = self.string(attrs, "tone", true)?.unwrap_or_default();
                if !matches!(tone, "note" | "tip" | "warning") {
                    return Err(self.reject_at(RejectCode::InvalidAttribute, "tone"));
                }
                self.out.push_str(r#","attrs":{"tone":"#);
                write_string(&mut self.out, tone);
                self.out.push('}');
            }
            // Nodes without attributes never get here: `attrs` is an unknown field for them.
            _ => {}
        }
        Ok(())
    }

    /// An integer attribute. `None` when it is optional and absent.
    fn integer(
        &self,
        attrs: &Map<String, Value>,
        name: &str,
        required: bool,
        range: std::ops::RangeInclusive<i64>,
    ) -> Result<Option<i64>, Rejection> {
        match attrs.get(name) {
            None if required => Err(self.reject_at(RejectCode::MissingField, name)),
            None => Ok(None),
            Some(Value::Number(number)) => match number.as_i64() {
                Some(value) if range.contains(&value) => Ok(Some(value)),
                // An integer, but out of range (or beyond i64): the right type, a wrong value.
                Some(_) => Err(self.reject_at(RejectCode::InvalidAttribute, name)),
                None if number.is_u64() => Err(self.reject_at(RejectCode::InvalidAttribute, name)),
                // A fraction such as 2.5 or 2.0: integers only.
                None => Err(self.reject_at(RejectCode::WrongType, name)),
            },
            Some(_) => Err(self.reject_at(RejectCode::WrongType, name)),
        }
    }

    /// A string attribute. `None` when it is optional and absent.
    fn string<'a>(
        &self,
        attrs: &'a Map<String, Value>,
        name: &str,
        required: bool,
    ) -> Result<Option<&'a str>, Rejection> {
        match attrs.get(name) {
            None if required => Err(self.reject_at(RejectCode::MissingField, name)),
            None => Ok(None),
            Some(Value::String(text)) => Ok(Some(text)),
            Some(_) => Err(self.reject_at(RejectCode::WrongType, name)),
        }
    }

    fn text(&mut self, node: &Map<String, Value>, in_code: bool) -> Result<(), Rejection> {
        let text = match node.get("text") {
            None => return Err(self.reject_at(RejectCode::MissingField, "text")),
            Some(Value::String(text)) if text.is_empty() => {
                return Err(self.reject_at(RejectCode::EmptyText, "text"));
            }
            Some(Value::String(text)) => text,
            Some(_) => return Err(self.reject_at(RejectCode::WrongType, "text")),
        };
        self.out.push_str(r#","text":"#);
        write_string(&mut self.out, text);

        let marks = match node.get("marks") {
            None => return Ok(()),
            Some(Value::Array(marks)) => marks,
            Some(_) => return Err(self.reject_at(RejectCode::WrongType, "marks")),
        };
        if marks.is_empty() {
            return Ok(());
        }
        self.pointer.push("marks");
        let result = self.marks(marks, in_code);
        self.pointer.pop();
        result
    }

    fn marks(&mut self, marks: &[Value], in_code: bool) -> Result<(), Rejection> {
        let mut seen: Vec<&str> = Vec::with_capacity(marks.len());
        // Each mark's canonical JSON with its rank in the editor schema: marks are a set, so they
        // are stored sorted by rank, as the editor emits them.
        let mut written: Vec<(usize, String)> = Vec::with_capacity(marks.len());
        for (index, mark) in marks.iter().enumerate() {
            self.pointer.push_index(index);
            let Value::Object(mark) = mark else {
                return Err(self.reject(RejectCode::WrongType));
            };
            let name = match mark.get("type") {
                None => return Err(self.reject_at(RejectCode::MissingField, "type")),
                Some(Value::String(name)) => name.as_str(),
                Some(_) => return Err(self.reject_at(RejectCode::WrongType, "type")),
            };
            let Some(rank) = MARK_ORDER.iter().position(|known| *known == name) else {
                return Err(self.reject_at(RejectCode::UnknownMark, "type"));
            };
            let combined_with_code = (name == "code" && !seen.is_empty()) || seen.contains(&"code");
            if in_code || seen.contains(&name) || combined_with_code {
                return Err(self.reject(RejectCode::InvalidMarks));
            }
            if name == "link" {
                self.unknown_fields(mark, &["type", "attrs"])?;
                let attrs = match mark.get("attrs") {
                    None => return Err(self.reject_at(RejectCode::MissingField, "attrs")),
                    Some(Value::Object(attrs)) => attrs,
                    Some(_) => return Err(self.reject_at(RejectCode::WrongType, "attrs")),
                };
                self.pointer.push("attrs");
                self.unknown_fields(attrs, &["href"])?;
                let href = self.string(attrs, "href", true)?.unwrap_or_default();
                if !is_allowed_href(href) {
                    return Err(self.reject_at(RejectCode::UnsafeLink, "href"));
                }
                self.pointer.pop();
                let mut json = String::from(r#"{"type":"link","attrs":{"href":"#);
                write_string(&mut json, href);
                json.push_str("}}");
                written.push((rank, json));
            } else {
                self.unknown_fields(mark, &["type"])?;
                let mut json = String::from(r#"{"type":"#);
                write_string(&mut json, name);
                json.push('}');
                written.push((rank, json));
            }
            seen.push(name);
            self.pointer.pop();
        }
        written.sort_unstable_by_key(|(rank, _)| *rank);
        self.out.push_str(r#","marks":["#);
        for (index, (_, mark)) in written.into_iter().enumerate() {
            if index > 0 {
                self.out.push(',');
            }
            self.out.push_str(&mark);
        }
        self.out.push(']');
        Ok(())
    }

    fn content(
        &mut self,
        kind: Kind,
        node: &Map<String, Value>,
        depth: usize,
    ) -> Result<(), Rejection> {
        let model = kind.model();
        if model == Model::Leaf {
            // `content` is an unknown field for leaves; checked already.
            return Ok(());
        }
        let children = match node.get("content") {
            None => &[][..],
            Some(Value::Array(children)) => children.as_slice(),
            Some(_) => return Err(self.reject_at(RejectCode::WrongType, "content")),
        };
        if children.is_empty() {
            if model.requires_content() {
                return Err(self.reject_at(RejectCode::InvalidContent, "content"));
            }
            return Ok(());
        }
        self.pointer.push("content");
        self.out.push_str(r#","content":["#);
        for (index, child) in children.iter().enumerate() {
            self.pointer.push_index(index);
            let child_kind = self.node_kind(child, depth + 1)?;
            if !model.allows(index, child_kind) {
                return Err(self.reject(RejectCode::InvalidContent));
            }
            if index > 0 {
                self.out.push(',');
            }
            self.node_body(child_kind, child, depth + 1, model == Model::CodeText)?;
            self.pointer.pop();
        }
        self.out.push(']');
        self.pointer.pop();
        Ok(())
    }
}

/// `^[a-z0-9][a-z0-9+#.-]{0,19}$`
fn is_code_language(language: &str) -> bool {
    let bytes = language.as_bytes();
    let Some((first, rest)) = bytes.split_first() else {
        return false;
    };
    bytes.len() <= 20
        && (first.is_ascii_lowercase() || first.is_ascii_digit())
        && rest.iter().all(|b| {
            b.is_ascii_lowercase() || b.is_ascii_digit() || matches!(b, b'+' | b'#' | b'.' | b'-')
        })
}

/// `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx` in lowercase hex.
fn is_lowercase_uuid(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() == 36
        && bytes.iter().enumerate().all(|(index, b)| {
            if matches!(index, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_digit() || (b'a'..=b'f').contains(b)
            }
        })
}

/// Whitespace as JavaScript's `trim()` sees it, near enough: Unicode `White_Space` plus the
/// byte-order mark, so the editor and the server agree on what "blank" alt text is.
fn is_blank(character: char) -> bool {
    character.is_whitespace() || character == '\u{feff}'
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::super::{MAX_BYTES, MAX_DEPTH, MAX_NODES, RejectCode, validate};

    fn doc(content: &Value) -> Value {
        json!({ "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": content } })
    }

    fn code_of(value: &Value) -> Option<(RejectCode, String)> {
        validate(value)
            .err()
            .map(|rejection| (rejection.code, rejection.pointer))
    }

    #[test]
    fn harmless_differences_are_canonicalized() {
        // Key order, whitespace, mark order and empty attrs/marks/content carry no meaning.
        let messy: Value = serde_json::from_str(
            r#"{ "doc": { "content": [ { "content": [ { "marks": [ { "type": "italic" }, { "type": "link", "attrs": { "href": "/a" } } ], "text": "x", "type": "text" } ], "type": "paragraph" }, { "type": "paragraph", "content": [] }, { "type": "orderedList", "attrs": {}, "content": [ { "type": "listItem", "content": [ { "type": "paragraph" } ] } ] }, { "type": "codeBlock", "attrs": {} }, { "type": "paragraph", "content": [ { "type": "text", "text": "y", "marks": [] } ] } ], "type": "doc" }, "version": 1, "schema": "colophon/rich-text" }"#,
        )
        .expect("valid JSON");
        let document = validate(&messy).expect("valid");
        assert_eq!(
            document.canonical_json(),
            r#"{"schema":"colophon/rich-text","version":1,"doc":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x","marks":[{"type":"link","attrs":{"href":"/a"}},{"type":"italic"}]}]},{"type":"paragraph"},{"type":"orderedList","content":[{"type":"listItem","content":[{"type":"paragraph"}]}]},{"type":"codeBlock"},{"type":"paragraph","content":[{"type":"text","text":"y"}]}]}}"#
        );
    }

    #[test]
    fn integers_must_be_integers() {
        let heading =
            |level: Value| doc(&json!([{ "type": "heading", "attrs": { "level": level } }]));
        assert_eq!(
            code_of(&heading(json!(2.5))),
            Some((RejectCode::WrongType, "/doc/content/0/attrs/level".into()))
        );
        assert_eq!(
            code_of(&heading(json!(2.0))),
            Some((RejectCode::WrongType, "/doc/content/0/attrs/level".into()))
        );
        assert_eq!(
            code_of(&heading(json!(-3))),
            Some((
                RejectCode::InvalidAttribute,
                "/doc/content/0/attrs/level".into()
            ))
        );
        assert_eq!(
            code_of(&heading(json!(u64::MAX))),
            Some((
                RejectCode::InvalidAttribute,
                "/doc/content/0/attrs/level".into()
            ))
        );
        assert!(validate(&heading(json!(4))).is_ok());
    }

    #[test]
    fn too_many_nodes_is_refused() {
        // A paragraph holding MAX_NODES line breaks: doc + paragraph + breaks > MAX_NODES.
        let breaks: Vec<Value> = (0..MAX_NODES)
            .map(|_| json!({ "type": "hardBreak" }))
            .collect();
        let value = doc(&json!([{ "type": "paragraph", "content": breaks }]));
        assert_eq!(
            code_of(&value),
            Some((RejectCode::TooManyNodes, String::new()))
        );
        let breaks: Vec<Value> = (0..MAX_NODES - 2)
            .map(|_| json!({ "type": "hardBreak" }))
            .collect();
        let value = doc(&json!([{ "type": "paragraph", "content": breaks }]));
        assert_eq!(
            validate(&value).map(|document| document.nodes()),
            Ok(MAX_NODES)
        );
    }

    #[test]
    fn too_large_is_refused() {
        let text = "x".repeat(MAX_BYTES);
        let value =
            doc(&json!([{ "type": "paragraph", "content": [{ "type": "text", "text": text }] }]));
        assert_eq!(code_of(&value), Some((RejectCode::TooLarge, String::new())));
    }

    #[test]
    fn hostile_nesting_is_refused_without_exhausting_the_stack() {
        use serde_json::Map;

        // 100,000 levels, built by hand: serde_json's parser stops at 128, and `json!` with a
        // variable inside re-serializes it recursively. The validator must give up at depth 17
        // instead of recursing all the way down. Dropping a Value this deep would overflow the
        // stack (drop glue recurses), so it is leaked on purpose.
        let object = |pairs: Vec<(&str, Value)>| {
            Value::Object(
                pairs
                    .into_iter()
                    .map(|(key, value)| (key.to_owned(), value))
                    .collect::<Map<_, _>>(),
            )
        };
        let mut node = object(vec![("type", Value::from("paragraph"))]);
        for _ in 0..100_000 {
            node = object(vec![
                ("type", Value::from("blockquote")),
                ("content", Value::Array(vec![node])),
            ]);
        }
        let doc = object(vec![
            ("type", Value::from("doc")),
            ("content", Value::Array(vec![node])),
        ]);
        let value = std::mem::ManuallyDrop::new(object(vec![
            ("schema", Value::from("colophon/rich-text")),
            ("version", Value::from(1)),
            ("doc", doc),
        ]));
        let (code, pointer) = code_of(&value).expect("refused");
        assert_eq!(code, RejectCode::TooDeep);
        assert_eq!(pointer, format!("/doc{}", "/content/0".repeat(MAX_DEPTH)));
    }

    #[test]
    fn rejections_display_the_code_and_where() {
        let value = doc(&json!([]));
        let rejection = validate(&value).expect_err("an empty document");
        assert_eq!(
            rejection.to_string(),
            "rich text rejected: invalid_content at /doc/content"
        );
        let rejection = validate(&json!([1])).expect_err("not an object");
        assert_eq!(
            rejection.to_string(),
            "rich text rejected: wrong_type at the document root"
        );
    }
}
