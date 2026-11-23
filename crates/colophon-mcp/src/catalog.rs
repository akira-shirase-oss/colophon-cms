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

use crate::api::{Backend, Request, new_idempotency_key};
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

/// The definitions to list, skipping any name a hand-written tool already uses.
pub fn definitions(taken: &[&str]) -> Vec<Value> {
    CATALOG
        .iter()
        .filter(|op| !taken.contains(&op.name.as_str()))
        .map(Op::definition)
        .collect()
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
    fn definition(&self) -> Value {
        let mut properties = Map::new();
        let mut required = Vec::new();
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
        .chain(["body", "if_match", "confirm"])
        .collect();
    if let Some(unknown) = args.keys().find(|key| !allowed.contains(&key.as_str())) {
        return Outcome::invalid(&op.name, &format!("unknown argument `{unknown}`"));
    }
    if op.confirm && args.get("confirm") != Some(&Value::Bool(true)) {
        return Outcome::needs_confirmation(op.effect(&args));
    }
    let path = match op.path(&api.project().to_string(), &args) {
        Ok(path) => path,
        Err(problem) => return Outcome::invalid(&op.name, &problem),
    };
    let key = new_idempotency_key();
    let body = args.get("body").cloned();
    if op.body.is_some() && body.is_none() {
        return Outcome::invalid(&op.name, "`body` is required");
    }
    let mut request = Request::new(op.method.clone(), path);
    if let Some(body) = body.as_ref() {
        request = request.json(body);
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
                    op.definition()["annotations"]["destructiveHint"],
                    false,
                    "{}",
                    op.name
                );
            }
            let def = op.definition();
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
            uuid::Uuid::nil()
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
}
