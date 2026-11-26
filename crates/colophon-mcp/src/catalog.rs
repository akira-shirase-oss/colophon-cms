//! Every project operation of the API as an MCP tool (D-247).
//!
//! The hand-written tools in [`crate::tools`] cover the common editorial path with friendly
//! arguments. Everything else a person can do in a project — publishing, schedules, releases and
//! rollbacks, media, members, roles, invitations, API tokens, the audience, settings — is offered
//! here, one tool per operation of the committed `OpenAPI` document (`docs/api/openapi.json`,
//! compiled in, so the list always matches the server it was built with). Deleting a project is
//! never offered.
//!
//! The person stays in charge: a tool that changes the live site, removes or revokes something,
//! or manages people and access is marked `destructiveHint` and answers with what it would do
//! until it is called again with `"confirm": true` — which the model must only send after the
//! person said yes. Every call is still decided by the API with the caller's own permissions.

use std::sync::LazyLock;

use reqwest::Method;
use serde_json::{Map, Value, json};

use crate::api::{Backend, Request, Upload, new_idempotency_key};
use crate::tools::Outcome;

/// The project-level routes (members, roles, tokens, invitations, audit, audience).
const PROJECT: &str = "/api/v1/projects/{project}/";

/// One operation offered as a tool.
#[derive(Debug, PartialEq, Eq)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "one flag per fact the document states about an operation"
)]
pub struct Op {
    pub name: String,
    title: String,
    summary: String,
    method: Method,
    /// The document's path template, `{name}` placeholders included.
    template: String,
    path_params: Vec<Param>,
    query_params: Vec<Param>,
    body: Option<Value>,
    /// The body is a file's bytes (an upload, D-251): the tool takes them base64-encoded, with
    /// the file's name and type.
    upload: bool,
    if_match: bool,
    idempotency: bool,
    read_only: bool,
    /// Needs the person's yes (`confirm: true`) before it is sent.
    confirm: bool,
    destructive: bool,
}

#[derive(Debug, PartialEq, Eq)]
struct Param {
    name: String,
    required: bool,
    schema: Value,
    description: String,
}

/// The document the server was built with.
static DOCUMENT: &str = include_str!("../../../docs/api/openapi.json");

/// The catalog, parsed once.
pub static CATALOG: LazyLock<Vec<Op>> = LazyLock::new(|| build(DOCUMENT));

/// The operation called `name`, if the catalog has one.
pub fn named(name: &str) -> Option<&'static Op> {
    CATALOG.iter().find(|op| op.name == name)
}

/// The definitions to list, skipping any name a hand-written tool already uses: `edit_entry`
/// (D-251), then every operation of the document.
pub fn definitions(taken: &[&str]) -> Vec<Value> {
    std::iter::once(edit_entry_definition(false))
        .chain(
            CATALOG
                .iter()
                .filter(|op| !taken.contains(&op.name.as_str()))
                .map(|op| op.definition(false)),
        )
        .collect()
}

/// The tools of a session that may act in every project of its person (D-248): `list_projects`,
/// then every catalog operation with a required `project` argument.
pub fn definitions_all_projects() -> Vec<Value> {
    let mut tools = vec![json!({
        "name": LIST_PROJECTS,
        "title": "List projects",
        "description": "Lists the projects you can work in, with your role in each. Every other tool takes one of these ids as `project`.",
        "inputSchema": {"type": "object", "properties": {}, "additionalProperties": false},
        "annotations": {"title": "List projects", "readOnlyHint": true, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
    })];
    tools.push(json!({
        "name": CREATE_PROJECT,
        "title": "Create project",
        "description": "Creates a project you own, within your account's project limit, and returns its id. Give it content types next (create_content_type), then entries.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "name": {"type": "string", "minLength": 1, "maxLength": 100, "description": "what people call the project, such as `Company website`"},
                "slug": {"type": "string", "description": "its URL name: lowercase letters and digits in groups joined by single hyphens, such as `company-website`"},
            },
            "required": ["name", "slug"],
            "additionalProperties": false,
        },
        "annotations": {"title": "Create project", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": false, "openWorldHint": false},
    }));
    tools.push(edit_entry_definition(true));
    tools.extend(CATALOG.iter().map(|op| op.definition(true)));
    tools
}

/// The project-creating tool of an all-projects session (D-251).
pub const CREATE_PROJECT: &str = "create_project";

/// `create_project`, for an all-projects session: the API holds it to the account's limit.
pub async fn create_project<B: Backend>(api: &B, arguments: Value) -> Outcome {
    let Value::Object(args) = arguments else {
        return Outcome::invalid(CREATE_PROJECT, "arguments must be an object");
    };
    if let Some(unknown) = args
        .keys()
        .find(|key| !matches!(key.as_str(), "name" | "slug"))
    {
        return Outcome::invalid(CREATE_PROJECT, &format!("unknown argument `{unknown}`"));
    }
    let (Some(Value::String(name)), Some(Value::String(slug))) =
        (args.get("name"), args.get("slug"))
    else {
        return Outcome::invalid(CREATE_PROJECT, "`name` and `slug` are required strings");
    };
    let body = json!({ "name": name, "slug": slug });
    let request = Request::new(Method::POST, "/api/v1/projects").json(&body);
    match api.send(&request).await {
        Ok(answer) => Outcome::from_answer(&answer),
        Err(error) => Outcome::from_api_error(CREATE_PROJECT, false, "", &error),
    }
}

/// The tool that changes some of a draft's fields and leaves the rest (D-251).
pub const EDIT_ENTRY: &str = "edit_entry";

/// Every argument `edit_entry` takes.
const EDIT_ENTRY_ARGUMENTS: [&str; 7] = [
    "project",
    "environment",
    "collection",
    "entry",
    "if_match",
    "set",
    "unset",
];

/// `edit_entry`'s definition; with `project` for an all-projects session.
pub fn edit_entry_definition(all_projects: bool) -> Value {
    let mut properties = Map::new();
    let mut required = Vec::new();
    if all_projects {
        properties.insert(
            "project".into(),
            json!({"type": "string", "format": "uuid", "description": "the project to act in: an id from list_projects"}),
        );
        required.push("project");
    }
    properties.insert(
        "environment".into(),
        json!({"type": "string", "default": "production", "description": "the environment's slug, such as `production`"}),
    );
    properties.insert(
        "collection".into(),
        json!({"type": "string", "description": "the collection's slug"}),
    );
    properties.insert(
        "entry".into(),
        json!({"type": "string", "format": "uuid", "description": "the entry's id"}),
    );
    properties.insert(
        "if_match".into(),
        json!({"type": "string", "description": "the draft's revision you read (its ETag, or revision.id), so you change exactly what you looked at"}),
    );
    properties.insert(
        "set".into(),
        json!({"type": "object", "description": "the fields to change, by field id, each with its whole new value (a list or an object field is replaced, not merged). Fields you leave out stay as they are"}),
    );
    properties.insert(
        "unset".into(),
        json!({"type": "array", "items": {"type": "string"}, "maxItems": 100, "uniqueItems": true, "description": "field ids to clear"}),
    );
    required.extend(["collection", "entry", "if_match"]);
    json!({
        "name": EDIT_ENTRY,
        "title": "Edit entry",
        "description": "Changes some fields of a draft and keeps every other field as it is, so a small fix never resends a long body: `set` the fields to change, `unset` the ones to clear. Saves a new revision on top of the one you read (`if_match`); if the draft changed since, nothing is saved. Publishes nothing. The API decides it with your own permissions.",
        "inputSchema": {"type": "object", "properties": properties, "required": required, "additionalProperties": false},
        "annotations": {"title": "Edit entry", "readOnlyHint": false, "destructiveHint": false, "idempotentHint": true, "openWorldHint": false},
    })
}

/// What an `edit_entry` call asks for, checked: the draft's path, the revision the caller read,
/// and the fields to change and to clear.
struct Edit {
    path: String,
    revision: String,
    set: Map<String, Value>,
    unset: Vec<String>,
}

/// One path segment of an `edit_entry` call: an id or a slug, or why it can't be one.
fn segment(
    args: &Map<String, Value>,
    name: &str,
    fallback: Option<&str>,
) -> Result<String, String> {
    let value = match (args.get(name), fallback) {
        (Some(Value::String(text)), _) => text.clone(),
        (None, Some(fallback)) => fallback.to_owned(),
        _ => return Err(format!("`{name}` is required")),
    };
    let safe = !value.is_empty()
        && value.len() <= 200
        && value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'));
    if safe {
        Ok(value)
    } else {
        Err(format!("`{name}` has characters an id or slug can't have"))
    }
}

/// Reads an `edit_entry` call's arguments; `project` is the session's own, or `None` when the
/// call must name one (an all-projects session).
fn edit_of(project: Option<uuid::Uuid>, args: &Map<String, Value>) -> Result<Edit, String> {
    if let Some(unknown) = args
        .keys()
        .find(|key| !EDIT_ENTRY_ARGUMENTS.contains(&key.as_str()))
    {
        return Err(format!("unknown argument `{unknown}`"));
    }
    let project = match project {
        Some(own) => own,
        None => args
            .get("project")
            .and_then(Value::as_str)
            .and_then(|id| uuid::Uuid::parse_str(id).ok())
            .ok_or("`project` must be a project id from list_projects")?,
    };
    let environment = segment(args, "environment", Some("production"))?;
    let collection = segment(args, "collection", None)?;
    let entry = segment(args, "entry", None)?;
    let Some(Value::String(revision)) = args.get("if_match") else {
        return Err("`if_match` is required: the revision you read".into());
    };
    let set = match args.get("set") {
        None | Some(Value::Null) => Map::new(),
        Some(Value::Object(fields)) => fields.clone(),
        Some(_) => return Err("`set` is an object of field ids and their new values".into()),
    };
    let unset = match args.get("unset") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(names)) if names.len() <= 100 => names
            .iter()
            .map(|name| name.as_str().map(str::to_owned))
            .collect::<Option<Vec<String>>>()
            .ok_or("`unset` is a list of field ids")?,
        Some(_) => return Err("`unset` is a list of at most 100 field ids".into()),
    };
    if set.is_empty() && unset.is_empty() {
        return Err("name at least one field in `set` or `unset`".into());
    }
    if let Some(both) = unset.iter().find(|name| set.contains_key(*name)) {
        return Err(format!("`{both}` is in both `set` and `unset`"));
    }
    Ok(Edit {
        path: format!(
            "/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries/{entry}"
        ),
        revision: revision.trim().trim_matches('"').to_owned(),
        set,
        unset,
    })
}

/// Runs `edit_entry`: reads the draft, changes the named fields in what it read, and saves that
/// on the revision the caller read. The save's `If-Match` is the caller's revision, so it lands
/// only when the draft read here is still that revision; otherwise the API's own stale-revision
/// refusal is the answer and nothing is written.
pub async fn edit_entry<B: Backend>(api: &B, arguments: Value) -> Outcome {
    let Value::Object(args) = arguments else {
        return Outcome::invalid(EDIT_ENTRY, "arguments must be an object");
    };
    let own = (!all_projects(api)).then(|| api.project());
    let edit = match edit_of(own, &args) {
        Ok(edit) => edit,
        Err(problem) => return Outcome::invalid(EDIT_ENTRY, &problem),
    };
    let read = match api.send(&Request::get(edit.path.clone())).await {
        Ok(answer) => answer,
        Err(error) => return Outcome::from_api_error(EDIT_ENTRY, true, "", &error),
    };
    let Some(Value::Object(current)) = read.body.pointer("/data/content") else {
        return Outcome::from_api_error(EDIT_ENTRY, true, "", &crate::api::ApiError::BadAnswer);
    };
    let mut content = current.clone();
    for name in &edit.unset {
        content.remove(name);
    }
    content.extend(edit.set);
    // Nothing to change: the draft already says this. Saving it again would add a revision with
    // the same content, and the entry would read as having unpublished changes it doesn't have.
    if content == *current {
        return Outcome::from_answer(&read);
    }
    let content = Value::Object(content);
    let key = new_idempotency_key();
    let request = Request::new(Method::PUT, edit.path)
        .json(&content)
        .if_match(&edit.revision)
        .idempotency_key(&key);
    match api.send(&request).await {
        Ok(answer) => Outcome::from_answer(&answer),
        Err(error) => Outcome::from_api_error(EDIT_ENTRY, false, &key, &error),
    }
}

const FILE_NAME: &str = "file_name";
const CONTENT_TYPE: &str = "content_type";
const CONTENT_BASE64: &str = "content_base64";

/// The largest file an upload tool can carry: what base64 leaves of one message, less room for
/// the rest of the call.
const MAX_UPLOAD_BYTES: usize = (crate::jsonrpc::MAX_MESSAGE_BYTES - 64 * 1024) / 4 * 3;

/// A file an upload tool was handed.
struct File {
    name: String,
    content_type: String,
    bytes: Vec<u8>,
}

/// The file in an upload tool's arguments, decoded; or what is wrong with them.
fn file_of(args: &Map<String, Value>) -> Result<File, String> {
    let text = |name: &str| match args.get(name) {
        Some(Value::String(text)) if !text.is_empty() => Ok(text.clone()),
        _ => Err(format!("`{name}` is required")),
    };
    let (name, content_type, encoded) =
        (text(FILE_NAME)?, text(CONTENT_TYPE)?, text(CONTENT_BASE64)?);
    // Header values: one line of visible ASCII, as HTTP takes them. The API checks the rest.
    for (field, value) in [(FILE_NAME, &name), (CONTENT_TYPE, &content_type)] {
        if value.len() > 255 || !value.bytes().all(|b| (0x20..0x7f).contains(&b)) {
            return Err(format!(
                "`{field}` is at most 255 plain ASCII characters, on one line"
            ));
        }
    }
    let bytes = decode_base64(&encoded).ok_or_else(|| {
        format!("`{CONTENT_BASE64}` isn't base64 (standard alphabet, padding optional)")
    })?;
    if bytes.is_empty() {
        return Err(format!("`{CONTENT_BASE64}` holds no bytes"));
    }
    Ok(File {
        name,
        content_type,
        bytes,
    })
}

/// Standard base64 (RFC 4648 §4), with or without padding, line breaks ignored. `None` for
/// anything else.
fn decode_base64(text: &str) -> Option<Vec<u8>> {
    use base64::engine::{DecodePaddingMode, Engine as _, GeneralPurpose, GeneralPurposeConfig};
    const LENIENT_PADDING: GeneralPurpose = GeneralPurpose::new(
        &base64::alphabet::STANDARD,
        GeneralPurposeConfig::new().with_decode_padding_mode(DecodePaddingMode::Indifferent),
    );
    let compact: String = text.chars().filter(|c| !matches!(c, '\n' | '\r')).collect();
    LENIENT_PADDING.decode(compact).ok()
}

/// The project-list tool of an all-projects session (D-248).
pub const LIST_PROJECTS: &str = "list_projects";

/// Whether this session may act in every project of its person rather than one (D-248): the
/// backend's project is the nil id then.
pub fn all_projects<B: Backend>(api: &B) -> bool {
    api.project().is_nil()
}

/// `list_projects`, for an all-projects session.
pub async fn list_projects<B: Backend>(api: &B) -> Outcome {
    match api.send(&Request::get("/api/v1/projects")).await {
        Ok(answer) => Outcome::from_answer(&answer),
        Err(error) => Outcome::from_api_error(LIST_PROJECTS, true, "", &error),
    }
}

fn snake(id: &str) -> String {
    let mut out = String::with_capacity(id.len() + 8);
    for (i, c) in id.chars().enumerate() {
        if c.is_ascii_uppercase() {
            if i > 0 {
                out.push('_');
            }
            out.push(c.to_ascii_lowercase());
        } else if c.is_ascii_alphanumeric() {
            out.push(c);
        } else {
            out.push('_');
        }
    }
    out
}

/// Words in a name or path that mean the call removes, undoes or takes something away.
const DESTRUCTIVE: [&str; 11] = [
    "delete",
    "remove",
    "revoke",
    "unpublish",
    "rollback",
    "cancel",
    "disable",
    "purge",
    "reset",
    "discard",
    "take_down",
];
/// Words that put something on the live site or decide for people: they need a yes too.
const DECIDES: [&str; 6] = [
    "publish", "approve", "schedule", "apply", "submit", "review",
];

fn build(document: &str) -> Vec<Op> {
    let Ok(doc) = serde_json::from_str::<Value>(document) else {
        return Vec::new();
    };
    let schemas = doc["components"]["schemas"].clone();
    let mut ops = Vec::new();
    let Some(paths) = doc["paths"].as_object() else {
        return ops;
    };
    for (path, item) in paths {
        if !path.starts_with(PROJECT) || path.ends_with("/mcp") || path.ends_with("/events") {
            continue;
        }
        let Some(item) = item.as_object() else {
            continue;
        };
        for (verb, operation) in item {
            let method = match verb.as_str() {
                "get" => Method::GET,
                "post" => Method::POST,
                "put" => Method::PUT,
                "patch" => Method::PATCH,
                "delete" => Method::DELETE,
                _ => continue,
            };
            let Some(id) = operation["operationId"].as_str() else {
                continue;
            };
            let name = snake(id);
            let mut path_params = Vec::new();
            let mut query_params = Vec::new();
            let (mut if_match, mut idempotency) = (false, false);
            for param in operation["parameters"].as_array().into_iter().flatten() {
                let param_name = param["name"].as_str().unwrap_or_default().to_owned();
                let one = Param {
                    name: param_name.clone(),
                    required: param["required"].as_bool().unwrap_or(false),
                    schema: plain(&param["schema"], &schemas, 0),
                    description: param["description"].as_str().unwrap_or_default().to_owned(),
                };
                match param["in"].as_str() {
                    Some("path") if param_name != "project" => path_params.push(one),
                    Some("query") => query_params.push(one),
                    Some("header") if param_name.eq_ignore_ascii_case("if-match") => {
                        if_match = true;
                    }
                    Some("header") if param_name.eq_ignore_ascii_case("idempotency-key") => {
                        idempotency = true;
                    }
                    _ => {}
                }
            }
            let body = operation["requestBody"]["content"]["application/json"]["schema"]
                .as_object()
                .map(|schema| plain(&Value::Object(schema.clone()), &schemas, 0));
            let upload =
                operation["requestBody"]["content"]["application/octet-stream"].is_object();
            let words = format!("{name} {}", path.to_ascii_lowercase().replace('-', "_"));
            // A read is never destructive, even one that previews a rollback.
            let destructive = method != Method::GET
                && (method == Method::DELETE || DESTRUCTIVE.iter().any(|w| words.contains(w)));
            let people = ["/members", "/roles", "/tokens", "/invitations", "/audience"]
                .iter()
                .any(|segment| path.contains(segment));
            let read_only = method == Method::GET;
            let confirm =
                !read_only && (destructive || people || DECIDES.iter().any(|w| name.contains(w)));
            let summary = operation["summary"]
                .as_str()
                .or_else(|| operation["description"].as_str())
                .unwrap_or(id)
                .to_owned();
            ops.push(Op {
                title: title_of(&name),
                name,
                summary,
                method,
                template: path.clone(),
                path_params,
                query_params,
                body,
                upload,
                if_match,
                idempotency,
                read_only,
                confirm,
                destructive,
            });
        }
    }
    ops.sort_by(|a, b| a.name.cmp(&b.name));
    ops.dedup_by(|a, b| a.name == b.name);
    ops
}

fn title_of(name: &str) -> String {
    let words = name.replace('_', " ");
    let mut chars = words.chars();
    chars
        .next()
        .map(|first| first.to_ascii_uppercase().to_string() + chars.as_str())
        .unwrap_or_default()
}

/// A schema a client can read on its own: references inlined a few levels deep, then a plain
/// object, so no `$ref` is left that points nowhere.
fn plain(schema: &Value, schemas: &Value, depth: usize) -> Value {
    match schema {
        Value::Object(map) => {
            if let Some(reference) = map.get("$ref").and_then(Value::as_str) {
                let name = reference.rsplit('/').next().unwrap_or_default();
                return if depth < 3 {
                    plain(&schemas[name], schemas, depth + 1)
                } else {
                    json!({ "type": "object", "description": format!("a {name}") })
                };
            }
            let mut out = Map::new();
            for (key, value) in map {
                if matches!(
                    key.as_str(),
                    "example" | "examples" | "xml" | "externalDocs"
                ) {
                    continue;
                }
                out.insert(key.clone(), plain(value, schemas, depth));
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(
            items
                .iter()
                .map(|item| plain(item, schemas, depth))
                .collect(),
        ),
        other => other.clone(),
    }
}

impl Op {
    fn definition(&self, all_projects: bool) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
        if all_projects {
            properties.insert(
                "project".into(),
                json!({"type": "string", "format": "uuid", "description": "the project to act in: an id from list_projects"}),
            );
            required.push(Value::String("project".into()));
        }
        for param in self.path_params.iter().chain(&self.query_params) {
            let mut schema = param.schema.clone();
            if let (Some(map), false) = (schema.as_object_mut(), param.description.is_empty()) {
                map.insert(
                    "description".into(),
                    Value::String(param.description.clone()),
                );
            }
            if param.name == "environment" {
                if let Some(map) = schema.as_object_mut() {
                    map.insert("default".into(), Value::String("production".into()));
                }
            } else if param.required {
                required.push(Value::String(param.name.clone()));
            }
            properties.insert(param.name.clone(), schema);
        }
        if let Some(body) = &self.body {
            properties.insert("body".into(), body.clone());
            required.push(Value::String("body".into()));
        }
        if self.upload {
            properties.insert(
                FILE_NAME.into(),
                json!({"type": "string", "minLength": 1, "maxLength": 255, "description": "the file's name, such as `team-photo.jpg`"}),
            );
            properties.insert(
                CONTENT_TYPE.into(),
                json!({"type": "string", "description": "the image type you claim for it, such as `image/png`, `image/jpeg` or `image/webp`; the server checks the bytes"}),
            );
            properties.insert(
                CONTENT_BASE64.into(),
                json!({"type": "string", "minLength": 1, "description": format!("the file's bytes, base64-encoded. One message carries at most {} KiB, so a file of up to about {} KiB fits; a larger one is uploaded in the Studio", crate::jsonrpc::MAX_MESSAGE_BYTES / 1024, MAX_UPLOAD_BYTES / 1024)}),
            );
            for name in [FILE_NAME, CONTENT_TYPE, CONTENT_BASE64] {
                required.push(Value::String(name.into()));
            }
        }
        if self.if_match {
            properties.insert(
                "if_match".into(),
                json!({"type": "string", "description": "the ETag/version you read (for example the entry's revision or the request's hash), so you change exactly what you looked at"}),
            );
        }
        if self.confirm {
            properties.insert(
                "confirm".into(),
                json!({"type": "boolean", "description": "true only after the person explicitly agreed to this exact action; without it nothing is sent and you get a description to show them"}),
            );
        }
        let mut description = self.summary.clone();
        if self.destructive {
            description.push_str(" DESTRUCTIVE: it removes, undoes or takes something away. Ask the person first, then call again with confirm: true.");
        } else if self.confirm {
            description.push_str(" It changes the live site or what people can do: ask the person first, then call again with confirm: true.");
        }
        description.push_str(" The API decides it with your own permissions.");
        json!({
            "name": self.name,
            "title": self.title,
            "description": description,
            "inputSchema": {
                "type": "object",
                "properties": properties,
                "required": required,
                "additionalProperties": false,
            },
            "annotations": {
                "title": self.title,
                "readOnlyHint": self.read_only,
                "destructiveHint": self.destructive || (self.confirm && !self.read_only),
                "idempotentHint": matches!(self.method, Method::GET | Method::PUT | Method::DELETE),
                "openWorldHint": false,
            },
        })
    }

    /// The request this call makes, or why not.
    fn path(&self, project: &str, args: &Map<String, Value>) -> Result<String, String> {
        let mut path = self.template.replace("{project}", project);
        for param in &self.path_params {
            let value = match args.get(&param.name) {
                Some(Value::String(text)) => text.clone(),
                Some(Value::Number(n)) => n.to_string(),
                None if param.name == "environment" => "production".into(),
                _ => return Err(format!("`{}` is required", param.name)),
            };
            let safe = !value.is_empty()
                && value.len() <= 200
                && value
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'));
            if !safe {
                return Err(format!(
                    "`{}` has characters an id or slug can't have",
                    param.name
                ));
            }
            path = path.replace(&format!("{{{}}}", param.name), &value);
        }
        let mut scratch =
            reqwest::Url::parse("http://query.invalid/").map_err(|_| "a query".to_owned())?;
        {
            let mut pairs = scratch.query_pairs_mut();
            for param in &self.query_params {
                match args.get(&param.name) {
                    None | Some(Value::Null) => {}
                    Some(Value::String(text)) => {
                        pairs.append_pair(&param.name, text);
                    }
                    Some(other) => {
                        pairs.append_pair(&param.name, &other.to_string());
                    }
                }
            }
        }
        if let Some(query) = scratch.query().filter(|query| !query.is_empty()) {
            path.push('?');
            path.push_str(query);
        }
        Ok(path)
    }

    /// What the call would do, in words for the person.
    fn effect(&self, args: &Map<String, Value>) -> String {
        let mut shown = args.clone();
        shown.remove("confirm");
        format!(
            "Not done yet. `{}` would {} {} with {}. {}Ask the person whether to go ahead; only if they say yes, call `{}` again with the same arguments and \"confirm\": true.",
            self.name,
            self.method.as_str(),
            self.template.trim_start_matches("/api/v1"),
            Value::Object(shown),
            if self.destructive {
                "This removes, undoes or takes something away. "
            } else {
                ""
            },
            self.name,
        )
    }
}

/// Runs one catalog tool.
pub async fn call<B: Backend>(api: &B, op: &'static Op, arguments: Value) -> Outcome {
    let Value::Object(args) = arguments else {
        return Outcome::invalid(&op.name, "arguments must be an object");
    };
    let allowed: Vec<&str> = op
        .path_params
        .iter()
        .chain(&op.query_params)
        .map(|param| param.name.as_str())
        .chain(["body", "if_match", "confirm", "project"])
        .chain(
            op.upload
                .then_some([FILE_NAME, CONTENT_TYPE, CONTENT_BASE64])
                .into_iter()
                .flatten(),
        )
        .collect();
    if let Some(unknown) = args.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Outcome::invalid(&op.name, &format!("unknown argument `{unknown}`"));
    }
    if op.confirm && args.get("confirm") != Some(&Value::Bool(true)) {
        return Outcome::needs_confirmation(op.effect(&args));
    }
    let project = if all_projects(api) {
        match args
            .get("project")
            .and_then(Value::as_str)
            .map(uuid::Uuid::parse_str)
        {
            Some(Ok(id)) => id.to_string(),
            _ => {
                return Outcome::invalid(
                    &op.name,
                    "`project` must be a project id from list_projects",
                );
            }
        }
    } else {
        api.project().to_string()
    };
    let path = match op.path(&project, &args) {
        Ok(path) => path,
        Err(problem) => return Outcome::invalid(&op.name, &problem),
    };
    let key = new_idempotency_key();
    let body = args.get("body").cloned();
    if op.body.is_some() && body.is_none() {
        return Outcome::invalid(&op.name, "`body` is required");
    }
    let file = if op.upload {
        match file_of(&args) {
            Ok(file) => Some(file),
            Err(problem) => return Outcome::invalid(&op.name, &problem),
        }
    } else {
        None
    };
    let mut request = Request::new(op.method.clone(), path);
    if let Some(body) = body.as_ref() {
        request = request.json(body);
    }
    if let Some(file) = file.as_ref() {
        request = request.upload(Upload {
            bytes: &file.bytes,
            file_name: &file.name,
            content_type: &file.content_type,
        });
    }
    if op.if_match
        && let Some(Value::String(tag)) = args.get("if_match")
    {
        // The bare value or the ETag as its header gave it, quotes and all: either names it.
        request = request.if_match(tag.trim().trim_matches('"'));
    }
    if op.idempotency {
        request = request.idempotency_key(&key);
    }
    match api.send(&request).await {
        Ok(answer) => Outcome::from_answer(&answer),
        Err(error) => Outcome::from_api_error(&op.name, op.read_only, &key, &error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_catalog_covers_the_project_and_never_deletes_one() {
        assert!(CATALOG.len() > 80, "only {} operations", CATALOG.len());
        assert!(CATALOG.iter().all(|op| op.template.starts_with(PROJECT)));
        assert!(
            !CATALOG.iter().any(
                |op| op.method == Method::DELETE && op.template == "/api/v1/projects/{project}"
            ),
            "deleting a project is never offered"
        );
        assert!(!CATALOG.iter().any(|op| op.template.ends_with("/mcp")));
    }

    #[test]
    fn writes_that_change_the_site_or_access_need_a_yes() {
        for op in CATALOG.iter() {
            if op.method == Method::DELETE {
                assert!(op.confirm && op.destructive, "{} must ask first", op.name);
            }
            if op.read_only {
                assert!(!op.confirm && !op.destructive, "{} only reads", op.name);
                assert_eq!(
                    op.definition(false)["annotations"]["destructiveHint"],
                    false,
                    "{}",
                    op.name
                );
            }
            let def = op.definition(false);
            assert!(
                op.definition(true)["inputSchema"]["required"]
                    .to_string()
                    .contains("project")
            );
            assert!(
                def["inputSchema"].to_string().find("$ref").is_none(),
                "{} leaves a $ref",
                op.name
            );
        }
        let publish = CATALOG
            .iter()
            .find(|op| op.name.contains("publish") && !op.read_only)
            .expect("a publish tool");
        assert!(publish.confirm);
    }

    /// A backend that counts what reaches it and answers nothing useful.
    struct Counting(std::sync::atomic::AtomicUsize);

    impl Backend for Counting {
        fn project(&self) -> uuid::Uuid {
            // A one-project session; the nil id would mean every project (D-248).
            uuid::Uuid::from_u128(0x0190_a000_0000_7000_8000_0000_0000_0001)
        }

        fn attempt<'a>(
            &'a self,
            _request: &'a Request<'a>,
            _deadline: tokio::time::Instant,
        ) -> impl Future<Output = Result<crate::api::Answer, crate::api::ApiError>> + Send + 'a
        {
            self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            std::future::ready(Err(crate::api::ApiError::Unreachable("counted".into())))
        }
    }

    #[tokio::test]
    async fn nothing_that_needs_a_yes_is_sent_without_one() {
        let backend = Counting(std::sync::atomic::AtomicUsize::new(0));
        let gated: Vec<&Op> = CATALOG.iter().filter(|op| op.confirm).collect();
        assert!(gated.len() > 20, "only {} need a yes", gated.len());
        for op in &gated {
            let mut args = Map::new();
            for param in &op.path_params {
                args.insert(
                    param.name.clone(),
                    json!("0190a000-0000-7000-8000-000000000000"),
                );
            }
            if op.body.is_some() {
                args.insert("body".into(), json!({}));
            }
            for confirm in [
                None,
                Some(json!(false)),
                Some(json!("true")),
                Some(json!(1)),
            ] {
                let mut args = args.clone();
                if let Some(value) = confirm {
                    args.insert("confirm".into(), value);
                }
                let outcome = call(&backend, op, Value::Object(args)).await;
                assert_eq!(
                    outcome.structured["needs_confirmation"],
                    json!(true),
                    "{}: {}",
                    op.name,
                    outcome.text
                );
                assert!(
                    outcome.text.contains(&op.name),
                    "{} says what it would do: {}",
                    op.name,
                    outcome.text
                );
            }
        }
        assert_eq!(
            backend.0.load(std::sync::atomic::Ordering::SeqCst),
            0,
            "a request was sent"
        );
        // And with the yes, it is sent: the gate is the only thing holding it.
        let op = gated[0];
        let mut args = Map::new();
        for param in &op.path_params {
            args.insert(
                param.name.clone(),
                json!("0190a000-0000-7000-8000-000000000000"),
            );
        }
        if op.body.is_some() {
            args.insert("body".into(), json!({}));
        }
        args.insert("confirm".into(), json!(true));
        let _ = call(&backend, op, Value::Object(args)).await;
        assert!(
            backend.0.load(std::sync::atomic::Ordering::SeqCst) > 0,
            "{} with confirm: true was not sent",
            op.name
        );
    }

    #[test]
    fn ids_and_slugs_are_safe_in_a_path() {
        let op = CATALOG
            .iter()
            .find(|op| !op.path_params.iter().all(|p| p.name == "environment"))
            .expect("one with an id");
        let mut args = Map::new();
        for param in &op.path_params {
            args.insert(param.name.clone(), Value::String("../../x".into()));
        }
        assert!(op.path("p", &args).is_err());
    }

    #[test]
    fn an_all_projects_session_lists_projects_and_names_one_per_call() {
        let tools = definitions_all_projects();
        assert_eq!(tools[0]["name"], LIST_PROJECTS);
        assert!(tools.len() > 80);
        assert!(tools.iter().all(|tool| tool["name"] != "delete_project"));
        assert!(tools.iter().any(|tool| tool["name"] == CREATE_PROJECT));
        assert!(tools.iter().any(|tool| tool["name"] == EDIT_ENTRY));
        // Every tool but the two that are about no one project takes a project.
        for tool in tools
            .iter()
            .filter(|tool| tool["name"] != LIST_PROJECTS && tool["name"] != CREATE_PROJECT)
        {
            let required = tool["inputSchema"]["required"].to_string();
            assert!(
                required.contains("\"project\""),
                "{} must take a project",
                tool["name"]
            );
        }
    }

    #[test]
    fn an_all_projects_path_names_any_project_but_never_leaves_the_projects() {
        let all = uuid::Uuid::nil();
        assert!(crate::api::full_path(all, "/api/v1/projects").is_ok());
        assert!(crate::api::full_path(all, "/api/v1/projects/abc/members").is_ok());
        assert!(crate::api::full_path(all, "/api/v1/projects/abc/../../setup").is_err());
        assert!(crate::api::full_path(all, "/api/v1/installation").is_err());
        assert!(crate::api::full_path(all, "/api/v1/accounts/x/disable").is_err());
        let one = uuid::Uuid::now_v7();
        assert!(
            crate::api::full_path(one, "/api/v1/projects").is_err(),
            "one project: no list"
        );
        assert!(
            crate::api::full_path(
                one,
                &format!("/api/v1/projects/{}/members", uuid::Uuid::now_v7())
            )
            .is_err()
        );
    }

    /// What reached a [`Recording`] backend: one request, owned.
    #[derive(Debug, Clone, PartialEq)]
    struct Sent {
        method: Method,
        path: String,
        body: Option<Value>,
        file: Option<(String, String, Vec<u8>)>,
        if_match: Option<String>,
        has_key: bool,
    }

    /// A backend that keeps what reaches it and answers from a list, in order; when the list
    /// runs out, the API is "unreachable".
    struct Recording {
        project: uuid::Uuid,
        sent: std::sync::Mutex<Vec<Sent>>,
        answers: std::sync::Mutex<std::collections::VecDeque<Value>>,
    }

    impl Recording {
        fn new(project: uuid::Uuid, answers: impl IntoIterator<Item = Value>) -> Self {
            Self {
                project,
                sent: std::sync::Mutex::default(),
                answers: std::sync::Mutex::new(answers.into_iter().collect()),
            }
        }

        fn sent(&self) -> Vec<Sent> {
            self.sent
                .lock()
                .expect("invariant: no test panics holding it")
                .clone()
        }
    }

    impl Backend for Recording {
        fn project(&self) -> uuid::Uuid {
            self.project
        }

        fn attempt<'a>(
            &'a self,
            request: &'a Request<'a>,
            _deadline: tokio::time::Instant,
        ) -> impl Future<Output = Result<crate::api::Answer, crate::api::ApiError>> + Send + 'a
        {
            self.sent
                .lock()
                .expect("invariant: no test panics holding it")
                .push(Sent {
                    method: request.method().clone(),
                    path: request.path().to_owned(),
                    body: request.body().cloned(),
                    file: request.upload_body().map(|file| {
                        (
                            file.file_name.to_owned(),
                            file.content_type.to_owned(),
                            file.bytes.to_vec(),
                        )
                    }),
                    if_match: request.if_match_tag().map(str::to_owned),
                    has_key: request.idempotency_key_value().is_some(),
                });
            let next = self
                .answers
                .lock()
                .expect("invariant: no test panics holding it")
                .pop_front();
            std::future::ready(next.map_or_else(
                || Err(crate::api::ApiError::Unreachable("no answer left".into())),
                |body| {
                    Ok(crate::api::Answer {
                        body,
                        replayed: false,
                    })
                },
            ))
        }
    }

    const ONE: uuid::Uuid = uuid::Uuid::from_u128(0x0190_a000_0000_7000_8000_0000_0000_0001);
    const ENTRY: &str = "0190a000-0000-7000-8000-0000000000e1";
    const REVISION: &str = "0190a000-0000-7000-8000-0000000000f1";

    #[tokio::test]
    async fn an_upload_sends_the_file_s_bytes_with_its_name_and_type() {
        let op = named("upload_asset").expect("the upload tool");
        let definition = op.definition(false);
        let required = definition["inputSchema"]["required"].to_string();
        for name in [FILE_NAME, CONTENT_TYPE, CONTENT_BASE64] {
            assert!(required.contains(name), "{name} is required: {required}");
        }
        assert!(named("replace_asset").expect("the replace tool").upload);

        let backend = Recording::new(ONE, [json!({"data": {"id": "a"}})]);
        // "PNG!" with and without its padding, and across a line break: all the same four bytes.
        for encoded in ["UE5HIQ==", "UE5HIQ", "UE5H\nIQ=="] {
            assert_eq!(
                decode_base64(encoded).as_deref(),
                Some(&b"PNG!"[..]),
                "{encoded}"
            );
        }
        let outcome = call(
            &backend,
            op,
            json!({"file_name": "dot.png", "content_type": "image/png", "content_base64": "UE5HIQ=="}),
        )
        .await;
        assert!(!outcome.is_error, "{}", outcome.text);
        let sent = backend.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(sent[0].method, Method::POST);
        assert!(
            sent[0].path.ends_with("/environments/production/assets"),
            "{}",
            sent[0].path
        );
        assert_eq!(
            sent[0].file,
            Some(("dot.png".into(), "image/png".into(), b"PNG!".to_vec()))
        );
        assert_eq!(sent[0].body, None, "the body is the file, not JSON");
        assert!(sent[0].has_key, "an upload carries an idempotency key");
    }

    #[tokio::test]
    async fn an_upload_that_isn_t_a_file_is_refused_before_anything_is_sent() {
        let op = named("upload_asset").expect("the upload tool");
        let backend = Recording::new(ONE, []);
        for arguments in [
            json!({}),
            json!({"file_name": "dot.png", "content_type": "image/png"}),
            json!({"file_name": "dot.png", "content_type": "image/png", "content_base64": "not base64!"}),
            json!({"file_name": "dot.png", "content_type": "image/png", "content_base64": "===="}),
            // A header value can't hold a line break: no second header rides in on a name.
            json!({"file_name": "dot.png\r\nX-Evil: 1", "content_type": "image/png", "content_base64": "UE5HIQ=="}),
            json!({"file_name": "dot.png", "content_type": "image/png\nx", "content_base64": "UE5HIQ=="}),
        ] {
            let outcome = call(&backend, op, arguments.clone()).await;
            assert!(outcome.is_error, "{arguments} was accepted");
        }
        assert!(backend.sent().is_empty(), "a request was sent");
        // A tool that isn't an upload doesn't take a file.
        let other = named("list_assets").expect("a read");
        let outcome = call(&backend, other, json!({"file_name": "x"})).await;
        assert!(outcome.is_error && backend.sent().is_empty());
    }

    #[tokio::test]
    async fn create_project_posts_the_name_and_slug_and_only_across_projects() {
        let backend = Recording::new(uuid::Uuid::nil(), [json!({"data": {"id": "p"}})]);
        let outcome = create_project(
            &backend,
            json!({"name": "Company website", "slug": "company-website"}),
        )
        .await;
        assert!(!outcome.is_error, "{}", outcome.text);
        let sent = backend.sent();
        assert_eq!(sent.len(), 1);
        assert_eq!(
            (&sent[0].method, sent[0].path.as_str()),
            (&Method::POST, "/api/v1/projects")
        );
        assert_eq!(
            sent[0].body,
            Some(json!({"name": "Company website", "slug": "company-website"}))
        );
        for arguments in [
            json!({}),
            json!({"name": "x"}),
            json!({"name": "x", "slug": 1}),
            json!({"name": "x", "slug": "x", "owner": "someone-else"}),
        ] {
            assert!(create_project(&backend, arguments).await.is_error);
        }
        assert_eq!(backend.sent().len(), 1, "a refused call sent something");
        // A one-project connection can't reach the project list at all (the path is refused).
        assert!(crate::api::full_path(ONE, "/api/v1/projects").is_err());
    }

    #[tokio::test]
    async fn edit_entry_changes_the_named_fields_and_keeps_the_rest() {
        let draft = json!({"data": {"id": ENTRY, "content": {
            "title": "Hello", "body": "a long body", "tags": ["a", "b"], "status": "old note"
        }}});
        let backend = Recording::new(ONE, [draft, json!({"data": {"id": ENTRY}})]);
        let outcome = edit_entry(
            &backend,
            json!({"collection": "posts", "entry": ENTRY, "if_match": format!("\"{REVISION}\""),
                   "set": {"title": "Hello again", "tags": ["c"]}, "unset": ["status"]}),
        )
        .await;
        assert!(!outcome.is_error, "{}", outcome.text);
        let sent = backend.sent();
        let path = format!(
            "/api/v1/projects/{ONE}/environments/production/collections/posts/entries/{ENTRY}"
        );
        assert_eq!(sent.len(), 2);
        assert_eq!(
            (&sent[0].method, sent[0].path.as_str()),
            (&Method::GET, path.as_str())
        );
        assert_eq!(
            (&sent[1].method, sent[1].path.as_str()),
            (&Method::PUT, path.as_str())
        );
        assert_eq!(
            sent[1].body,
            Some(json!({"title": "Hello again", "body": "a long body", "tags": ["c"]})),
            "set replaces whole values, unset clears, everything else is what was read"
        );
        // The save stands on the revision the caller read, not on whatever the read found.
        assert_eq!(
            sent[1].if_match.as_deref(),
            Some(format!("\"{REVISION}\"").as_str())
        );
        assert!(sent[1].has_key);
    }

    #[tokio::test]
    async fn edit_entry_saves_nothing_when_the_draft_already_says_it() {
        let draft = json!({"data": {"id": ENTRY, "content": {"title": "Hello", "tags": ["a"]}}});
        let backend = Recording::new(ONE, [draft.clone()]);
        let outcome = edit_entry(
            &backend,
            json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION,
                   "set": {"title": "Hello", "tags": ["a"]}, "unset": ["note"]}),
        )
        .await;
        assert!(!outcome.is_error, "{}", outcome.text);
        assert_eq!(outcome.structured, draft, "it answers the draft as it is");
        let sent = backend.sent();
        assert_eq!(sent.len(), 1, "only the read: {sent:?}");
        assert_eq!(sent[0].method, Method::GET);
    }

    #[tokio::test]
    async fn edit_entry_refuses_what_it_can_t_mean_before_anything_is_sent() {
        let backend = Recording::new(ONE, []);
        for arguments in [
            json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION}),
            json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION, "set": {}, "unset": []}),
            json!({"collection": "posts", "entry": ENTRY, "set": {"title": "x"}}),
            json!({"collection": "posts", "entry": "../../x", "if_match": REVISION, "set": {"title": "x"}}),
            json!({"collection": "posts/../roles", "entry": ENTRY, "if_match": REVISION, "set": {"title": "x"}}),
            json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION, "set": {"title": "x"}, "unset": ["title"]}),
            json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION, "set": ["title"]}),
            json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION, "set": {"title": "x"}, "body": {}}),
        ] {
            let outcome = edit_entry(&backend, arguments.clone()).await;
            assert!(outcome.is_error, "{arguments} was accepted");
        }
        assert!(backend.sent().is_empty(), "a request was sent");
        // Across projects it names its project, and nothing is saved when the read fails.
        let all = Recording::new(uuid::Uuid::nil(), []);
        let no_project = json!({"collection": "posts", "entry": ENTRY, "if_match": REVISION, "set": {"title": "x"}});
        assert!(edit_entry(&all, no_project).await.is_error);
        assert_eq!(all.sent().len(), 0, "a request was sent without a project");
        let unreachable = edit_entry(
            &all,
            json!({"project": ONE.to_string(), "collection": "posts", "entry": ENTRY, "if_match": REVISION, "set": {"title": "x"}}),
        )
        .await;
        assert!(unreachable.is_error);
        // The read is tried again while the API can't be reached; no save ever follows it.
        assert_ne!(all.sent().len(), 0, "the read never went out");
        assert!(
            all.sent().iter().all(|sent| sent.method == Method::GET),
            "something other than the read went out"
        );
    }
}
