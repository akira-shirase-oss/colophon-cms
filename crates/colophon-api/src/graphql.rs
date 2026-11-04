//! `POST {P}/collections/{collection}/graphql` — GraphQL over the same read the bounded
//! query language runs (DX.10, D-115).
//!
//! A mature executor — `async-graphql`'s dynamic schema — does the parsing, validation,
//! fragments, aliases, variables and introspection GraphQL itself specifies; what a read
//! *does* is Colophon's. The schema is built per request from the project's content
//! types, holding one node type per collection the caller's role may read:
//!
//! - `entries(where:, orderBy:, first:, after:)` answers a connection whose `nodes` are
//!   the page [`entries::run_query`] answers: the arguments and the selection compile
//!   to the *same* canonical query document `POST …/entries/query` takes (D-103), which
//!   `Document::validate` holds to every bound — indexed fields only, `MAX_FILTERS`,
//!   `MAX_SORT_KEYS`, `limit` 1–100, `depth` 0–2 — and the same pipeline runs it:
//!   `whose` narrowing, the D-109 field-leak refusal, `db_query::page`, the role's
//!   field projection and the bounded batched population (D-107). `applied` echoes the
//!   canonical document and `sha256:<hex>` hash exactly as REST's does, and `pageInfo`'s
//!   `endCursor` is the `next_cursor` a `?cursor=` would take.
//! - `entry(id:)` reads one draft like `GET …/entries/{entry}` does: an entry the role
//!   can't read is `null`, as if it didn't exist.
//!
//! Nested selections on a single-reference field become `depth`: selecting `id` or
//! `collection` alone reads the stored id for free — a populated object carries both —
//! while any deeper selection costs one level, so `author { mentor { title } }` is a
//! `depth` of 2 and a third level is refused `out_of_range`, the code the document
//! itself answers with.
//!
//! GraphQL refuses nothing the document wouldn't: every refusal — a filter on an
//! unreadable field, a cursor that isn't this query's, a bound exceeded — is a GraphQL
//! error on the field that ran, `message` the refusal's own and `extensions.code` /
//! `extensions.pointer` carrying its `code` and JSON Pointer so clients branch on the
//! same values REST's envelope holds. Errors before a document exists — a missing
//! collection, a role that can't read it, a body that isn't the GraphQL request —
//! answer the API envelope as any route does. Reads are the whole surface: mutations
//! stay REST, and delivery — pinned generations, its own population — is out of scope.
//!
//! Bounds beyond the document's own (the schema bounds what a selection can name):
//! the document is at most [`MAX_QUERY_CHARS`], the root takes at most
//! [`MAX_ROOT_READS`] `entries`/`entry` calls — aliased ones included — and a `nodes`
//! or `entry` subtree may name at most [`MAX_NODE_SELECTIONS`] fields, the alias bound.

use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

use async_graphql::{
    Error, ErrorExtensions, Name, Request, Variables,
    dynamic::{
        Enum, Field, FieldFuture, FieldValue, InputObject, InputValue, Object, ResolverContext,
        Scalar, Schema, TypeRef,
    },
};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::Response,
};
use colophon_db::{content_types, entries as db_entries, scope::ProjectTransaction};
use colophon_domain::{
    policy::Action,
    schema::{ContentType, FieldKind, Refusal},
};
use serde::Deserialize;
use serde_json::{Map, Value, json};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    entries::{self, CollectionPath, content_type, json, may_read, narrow, open},
    session::{SignedIn, services},
    visibility::{self, Access},
};

/// The GraphQL document a request carries is at most this long, so a query is a bounded
/// string before the parser sees it (the body's own limit stays the outer bound).
const MAX_QUERY_CHARS: usize = 8 * 1024;
/// `entries` and `entry` root calls per request, aliased ones included: each runs a
/// bounded page, and a request holds at most this many.
const MAX_ROOT_READS: usize = 8;
/// Selections — each occurrence, aliases counted — inside one `nodes` or `entry`
/// subtree: the response-shape bound, beside the document's own.
const MAX_NODE_SELECTIONS: usize = 128;

/// The fields a node type answers that aren't content fields. A content field whose id
/// names one is renamed `field_<id>` so both stay selectable.
const NODE_META: &[&str] = &[
    "id",
    "collection",
    "revisionId",
    "title",
    "hiddenFields",
    "published",
    "locale",
    "createdAt",
    "updatedAt",
    "createdBy",
    "content",
];

/// Selections an unresolved reference answers without expansion: `id` is the stored
/// value itself and `collection` the target the field declares — anything more costs a
/// `depth` level.
const REFERENCE_FREE: &[&str] = &["id", "collection"];

/// Names GraphQL keeps for itself or this schema generates; a collection whose type
/// name lands on one gets `Entry` appended (collection slugs are lowercase, so a
/// generated `*Connection` or scalar name can never collide).
const RESERVED_TYPES: &[&str] = &[
    "Query",
    "Mutation",
    "Subscription",
    "PageInfo",
    "AppliedQuery",
    "Publication",
    "SortDirection",
    "JSON",
    "query",
    "mutation",
    "subscription",
    "schema",
    "type",
    "scalar",
    "interface",
    "union",
    "enum",
    "input",
    "directive",
    "extend",
    "implements",
    "on",
    "fragment",
    "true",
    "false",
    "null",
];

/// A GraphQL document in the request body, per the over-HTTP convention:
/// `{"query": "…", "operationName": "…", "variables": {…}}`. Only those keys — like
/// every body the API takes, nothing else is silently dropped.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct GraphQLBody {
    /// The document: `{ entries(first: 10) { nodes { … } } }`.
    query: String,
    /// Which of its operations runs, when the document names several.
    #[serde(rename = "operationName", default)]
    operation_name: Option<String>,
    /// Variable values; an object.
    #[serde(default)]
    variables: Option<Value>,
}

/// `POST {P}/collections/{collection}/graphql`: the dynamic schema over the caller's
/// readable collections, one bounded `run_query` per `entries` call inside one
/// transaction. The response is GraphQL's own `{data, errors}` — never the error
/// envelope — while the document runs at all; what stops one earlier is the envelope
/// any route answers.
pub(crate) async fn graphql(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<CollectionPath>,
    body: Result<Json<GraphQLBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(body) = body?;
    if body.query.chars().count() > MAX_QUERY_CHARS {
        return Err(ApiError::Invalid(Refusal {
            code: "too_large",
            pointer: "/query".to_owned(),
            message: format!("a GraphQL document is at most {MAX_QUERY_CHARS} characters"),
        }));
    }
    // `variables` is a JSON object when present; `Variables` says the same of what it holds.
    let variables = match body.variables {
        None => Variables::default(),
        Some(Value::Object(object)) => Variables::from_json(Value::Object(object)),
        Some(_) => {
            return Err(ApiError::Invalid(Refusal {
                code: "wrong_type",
                pointer: "/variables".to_owned(),
                message: "`variables` is an object of variable names to values".to_owned(),
            }));
        }
    };
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    may_read(&tx, &path.collection)?;
    let (content_type, _) = content_type(&state, &mut tx, &path.collection).await?;

    // Every collection the role may read some entries of — own-only counts — gets a
    // node type, so references can name their target's fields; a reference whose `to`
    // the role can't read resolves `ID` instead, honest about the string it holds.
    let mut definitions = HashMap::new();
    for version in content_types::list(&mut tx)
        .await
        .map_err(ApiError::from_db)?
    {
        let readable = [false, true].into_iter().any(|own| {
            tx.policy
                .fields(Action::ContentRead, &version.collection, own)
                .is_some()
        });
        if readable {
            definitions.insert(version.collection, version.definition);
        }
    }
    let defs = defs_for(
        &services(&state)?.content_types,
        &definitions,
        &path.collection,
        &content_type,
    )?;

    let tx = Arc::new(Mutex::new(Some(tx)));
    let context = Context {
        state: state.clone(),
        tx: Arc::clone(&tx),
        collection: path.collection.clone(),
        content_type,
        defs,
        reads: AtomicUsize::new(0),
    };
    let mut request = Request::new(body.query).variables(variables);
    if let Some(name) = body.operation_name {
        request = request.operation_name(name);
    }
    let schema = build_schema(context)?;
    let response = schema.execute(request).await;
    let body = serde_json::to_string(&response).map_err(ApiError::internal)?;

    // Whatever the resolvers ran, it read one snapshot; nothing was written, but the
    // transaction commits like every read's does.
    let taken = tx.lock().await.take().ok_or_else(|| {
        ApiError::internal(std::io::Error::other("the GraphQL transaction was dropped"))
    })?;
    taken.commit().await.map_err(ApiError::from_db)?;
    Ok(json(StatusCode::OK, body, None, false))
}

/// What a request's resolvers share: the open transaction — behind a lock since the
/// executor may resolve sibling fields together — and everything schema-build learned.
struct Context {
    /// For `resolve_drafts`, which needs the pool beside `tx`.
    state: AppState,
    /// The caller's open transaction; `Some` until the handler takes it to commit.
    tx: Arc<Mutex<Option<ProjectTransaction>>>,
    /// The collection the endpoint is scoped to — the type `entries` and `entry` answer.
    collection: String,
    /// Its parsed definition, the document's validator.
    content_type: Arc<ContentType>,
    /// Every readable collection's GraphQL shape.
    defs: Arc<Definitions>,
    /// Root calls run so far; [`MAX_ROOT_READS`] bounds them.
    reads: AtomicUsize,
}

/// Every collection's GraphQL shape: its node type's name and a lookup from a GraphQL
/// field name to the content field it resolves — the renamed `field_<id>` included.
struct Definitions {
    /// By collection slug.
    by_collection: HashMap<String, Arc<CollectionShape>>,
}

/// One collection's GraphQL shape.
struct CollectionShape {
    /// The collection slug.
    collection: String,
    /// Its node type's name: the slug sanitized — hyphens and such to `_` — or with
    /// `Entry` appended where the name is GraphQL's own.
    type_name: String,
    /// GraphQL field name → the declared field it answers.
    fields: BTreeMap<String, GraphQLField>,
    /// The parsed definition; resolvers take field kinds and order from it.
    definition: Arc<ContentType>,
}

/// A content field as GraphQL knows it: its name — the id, or `field_<id>` where the id
/// is a node's meta name — and how a node's stored value answers it.
#[derive(Debug, Clone)]
struct GraphQLField {
    /// The field id stored content keys by.
    id: String,
    /// How a stored value becomes the field's answer.
    resolve: Resolve,
}

/// What a node's stored value for one field means to GraphQL.
#[derive(Debug, Clone)]
enum Resolve {
    /// A stored string: text, slug, email, URL, markdown, color, enum, date, time,
    /// datetime — or a single asset's id.
    Text,
    /// A stored number.
    Number,
    /// A stored boolean.
    Boolean,
    /// The stored JSON verbatim: `json`, `richtext`, `object`, `list`, `blocks`.
    Json,
    /// A list of id strings: `asset { many: true }` or `reference { many: true }`.
    IdList,
    /// A single reference whose target collection the role can't read: the stored id,
    /// typed `ID` — it can never expand, so nothing promises object fields it lacks.
    Id,
    /// A single reference whose target the role may read: the stored id — or the
    /// `Expanded` object a `depth` resolution wrote — becomes the target's node type.
    Reference {
        /// The target collection, for the unresolved node's `collection`.
        to: String,
    },
}

/// A node the connection or a reference answers: everything its resolvers need of the
/// row. An unresolved reference is a `Node` of `id` and `collection` alone — `content`
/// `None` — which every content-field resolver answers `null` for, so `{ id }` and
/// `{ collection }` on it still work.
#[derive(Clone)]
struct Node {
    /// The entry's id.
    id: String,
    /// Its collection.
    collection: String,
    /// The current draft revision's id — `None` on an unresolved reference.
    revision_id: Option<String>,
    /// Its title, blanked where the role can't read the field it comes from.
    title: Option<String>,
    /// The narrowed (and possibly expanded) document.
    content: Option<Map<String, Value>>,
    /// The type's fields the role can't read; `[]` when it reads them all.
    hidden_fields: Option<Vec<String>>,
    /// The live revision and its generation, on the draft perspective only.
    published: Option<Publication>,
    /// Its locale tag.
    locale: Option<String>,
    /// Timestamps and the creator, on the draft perspective only.
    created_at: Option<String>,
    /// `updated_at`.
    updated_at: Option<String>,
    /// `created_by`.
    created_by: Option<String>,
}

/// `published` of a draft node.
#[derive(Clone)]
struct Publication {
    /// The live revision.
    revision_id: String,
    /// The release generation that published it.
    generation: i64,
}

/// The value `pageInfo`'s resolvers actually see — a tiny copy of the connection's
/// cursor state, since `FieldValue` hands children owned values.
struct PageState {
    /// `connection.end_cursor`.
    end_cursor: Option<String>,
    /// `connection.has_next`.
    has_next: bool,
}

/// The connection `entries` answers: `pageInfo` beside the nodes, `applied` the query
/// that ran — REST's own shape, so a client can port a call to `entries/query` one to
/// one.
struct Connection {
    /// The page's nodes.
    nodes: Vec<Node>,
    /// Where the next page begins.
    end_cursor: Option<String>,
    /// Whether `end_cursor` names a page beyond this one.
    has_next: bool,
    /// The canonical document and its hash.
    applied: Applied,
}

/// `applied`: the canonical query JSON and `sha256:<hex>` — what `entries/query`
/// echoes, so a GraphQL call and its REST twin prove they ran one document.
struct Applied {
    /// `query.canonical_json()`, as JSON.
    query: Value,
    /// `sha256:<hex>` of those bytes.
    query_hash: String,
}

// `POST /graphql`'s handler ends here; the rest builds and runs the schema.

/// The schema field name a content field gets: its id, or `field_<id>` where the id is
/// a node's meta name — `field_` prefixed again, once more, until the name is free.
fn gql_name(id: &str, declared: &BTreeSet<String>) -> String {
    if !NODE_META.contains(&id) {
        return id.to_owned();
    }
    let mut candidate = format!("field_{id}");
    while declared.contains(&candidate) || NODE_META.contains(&candidate.as_str()) {
        candidate = format!("field_{candidate}");
    }
    candidate
}

/// The node's content-field resolution: `node.content`'s `id` member as the `resolve`
/// kind shapes it — `None` (the GraphQL `null`) when the node is an unresolved
/// reference or the field isn't stored.
fn node_field<'a>(node: &Node, field: &GraphQLField) -> Result<Option<FieldValue<'a>>, Error> {
    let Some(content) = &node.content else {
        return Ok(None);
    };
    let Some(value) = content.get(&field.id) else {
        return Ok(None);
    };
    if value.is_null() {
        return Ok(None);
    }
    let resolved = match &field.resolve {
        Resolve::Text | Resolve::Id => value
            .as_str()
            .map(|text| FieldValue::value(async_graphql::Value::from(text.to_owned()))),
        Resolve::Number => value.as_f64().map(FieldValue::value),
        Resolve::Boolean => value.as_bool().map(FieldValue::value),
        Resolve::Json => Some(FieldValue::value(
            async_graphql::Value::from_json(value.clone()).map_err(internal)?,
        )),
        Resolve::IdList => Some(FieldValue::list(
            value
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(|id| FieldValue::value(async_graphql::Value::from(id.to_owned()))),
        )),
        Resolve::Reference { to } => Some(reference_node(to, value)),
    };
    Ok(resolved)
}

/// The node a single-reference value answers: an `Expanded` object (`depth` resolved
/// it) becomes a populated node of the target collection; anything else — the stored
/// id, or a value expansion couldn't resolve — is `{id, collection}` alone.
fn reference_node<'a>(to: &str, value: &Value) -> FieldValue<'a> {
    let Value::Object(object) = value else {
        return match value.as_str() {
            Some(id) => FieldValue::owned_any(Node::unresolved(id, to)),
            None => FieldValue::NULL,
        };
    };
    FieldValue::owned_any(Node {
        id: object
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        collection: object
            .get("collection")
            .and_then(Value::as_str)
            .unwrap_or(to)
            .to_owned(),
        revision_id: object
            .get("revision_id")
            .and_then(Value::as_str)
            .map(str::to_owned),
        title: object
            .get("title")
            .and_then(Value::as_str)
            .map(str::to_owned),
        content: object.get("content").and_then(Value::as_object).cloned(),
        hidden_fields: object
            .get("hidden_fields")
            .and_then(Value::as_array)
            .map(|list| {
                list.iter()
                    .filter_map(|field| field.as_str().map(str::to_owned))
                    .collect()
            }),
        published: None,
        locale: None,
        created_at: None,
        updated_at: None,
        created_by: None,
    })
}

impl Node {
    /// The node a stored-but-unresolved reference is: `id` and `collection`, every
    /// other member `None` — content-field resolvers answer `null` on it.
    fn unresolved(id: &str, to: &str) -> Self {
        Self {
            id: id.to_owned(),
            collection: to.to_owned(),
            revision_id: None,
            title: None,
            content: None,
            hidden_fields: None,
            published: None,
            locale: None,
            created_at: None,
            updated_at: None,
            created_by: None,
        }
    }
}

/// One selected field as the compiler needs it: the schema name and its children —
/// aliases already resolved (the executor names fields), `@skip`/`@include` honored.
#[derive(Debug, Clone)]
struct Selected {
    /// The field's name on the type.
    name: String,
    /// Its subselections.
    children: Vec<Selected>,
}

/// The canonical document a selection and its arguments compile to, plus the `keep`
/// the narrowing applies — `fields` is part of `document`, kept beside it for
/// `entry`'s projection use.
#[derive(Debug)]
struct Compiled {
    /// The document `Document::validate` checks: `filters`, `sort`, `limit`, `fields`,
    /// `depth` — exactly the members the REST body holds.
    document: Value,
    /// `document["fields"]` as a list, when the selection named declared fields —
    /// `entry` needs it without validating a document at all.
    keep: Option<Vec<String>>,
}

/// The selection and `entries`/`entry` arguments, compiled to the document and `depth`
/// it implies. `filters_in` is the `where` object's JSON (`{"<field>": {"<op>": v}}`),
/// `sort_in` the `orderBy` list (`[{"field": "…", "dir": "ASC"}]`).
fn compile(
    defs: &Definitions,
    shape: &CollectionShape,
    selected: &[Selected],
    filters_in: Option<&Map<String, Value>>,
    sort_in: Option<&Vec<Value>>,
    first: Option<i64>,
) -> Result<Compiled, Refusal> {
    let mut filters = Vec::new();
    if let Some(object) = filters_in {
        for (name, ops) in object {
            // The input type only carries declared indexed fields — a name it lacks
            // can't reach here; a meta name isn't a field, simply skipped.
            let Some(field) = shape.fields.get(name.as_str()) else {
                continue;
            };
            let Some(ops) = ops.as_object() else {
                continue;
            };
            for (op, value) in ops {
                filters.push(json!({"field": field.id, "op": op, "value": value}));
            }
        }
    }
    let mut sort = Vec::new();
    if let Some(list) = sort_in {
        for item in list {
            let Some(object) = item.as_object() else {
                continue;
            };
            let Some(field) = object
                .get("field")
                .and_then(Value::as_str)
                .and_then(|name| shape.fields.get(name))
            else {
                continue;
            };
            let dir = match object.get("dir").and_then(Value::as_str) {
                Some("DESC") => "desc",
                _ => "asc",
            };
            sort.push(json!({"field": field.id, "dir": dir}));
        }
    }
    // The node subtree's bound: every occurrence counts, so aliases can't multiply it.
    if count_selected(selected) > MAX_NODE_SELECTIONS {
        return Err(Refusal {
            code: "too_many",
            pointer: String::new(),
            message: format!("a selection names at most {MAX_NODE_SELECTIONS} fields"),
        });
    }
    let mut keep = Vec::new();
    for selection in selected {
        if let Some(field) = shape.fields.get(selection.name.as_str()) {
            keep.push(field.id.clone());
        }
    }
    keep.sort();
    keep.dedup();
    // `depth_of` counts the reference levels the subtree needs; `Document::validate`
    // bounds the result to MAX_DEPTH itself — the `out_of_range` a too-deep document
    // refuses with is the same refusal here.
    let depth = depth_of(defs, &shape.collection, selected);
    let mut document = Map::new();
    if !filters.is_empty() {
        document.insert("filters".to_owned(), Value::Array(filters));
    }
    if !sort.is_empty() {
        document.insert("sort".to_owned(), Value::Array(sort));
    }
    if let Some(limit) = first {
        document.insert("limit".to_owned(), Value::from(limit));
    }
    if !keep.is_empty() {
        document.insert("fields".to_owned(), json!(keep));
    }
    document.insert("depth".to_owned(), Value::from(depth));
    Ok(Compiled {
        document: Value::Object(document),
        keep: (!keep.is_empty()).then_some(keep),
    })
}

/// The `depth` `selected` asks for: a single-reference field whose children name
/// anything beyond `id`/`collection` costs one level, plus whatever the target's own
/// subtree costs — `author { mentor { title } }` is two. The document's own bound
/// applies the refusal, so this saturates rather than wraps.
fn depth_of(defs: &Definitions, collection: &str, selected: &[Selected]) -> u8 {
    let Some(shape) = defs.by_collection.get(collection) else {
        return 0;
    };
    selected
        .iter()
        .filter_map(|selection| {
            let field = shape.fields.get(selection.name.as_str())?;
            let Resolve::Reference { to } = &field.resolve else {
                return None;
            };
            wants_expansion(selection)
                .then(|| depth_of(defs, to, &selection.children).saturating_add(1))
        })
        .max()
        .unwrap_or(0)
}

/// Whether resolving `selection`'s children needs the reference populated: only `id`
/// and `collection` are free, both present on the unresolved value.
fn wants_expansion(selection: &Selected) -> bool {
    selection
        .children
        .iter()
        .any(|child| !REFERENCE_FREE.contains(&child.name.as_str()))
}

/// The occurrences `selected` holds, root and below — the [`MAX_NODE_SELECTIONS`]
/// check, where aliased duplicates count.
fn count_selected(selected: &[Selected]) -> usize {
    selected
        .iter()
        .map(|selection| 1 + count_selected(&selection.children))
        .sum()
}

/// The `Selected` tree of a `nodes` or `entry` field's selection set, `@skip(if: …)`
/// and `@include(if: …)` honored — a `@skip`'d branch can't cost depth it never runs.
fn collect(field: &async_graphql::SelectionField<'_>) -> Vec<Selected> {
    field
        .selection_set()
        .filter(|child| !skipped(child))
        .map(|child| Selected {
            name: child.name().to_owned(),
            children: collect(&child),
        })
        .collect()
}

/// Whether `@skip`/`@include` on `field` leaves it out — `directives()` evaluates
/// `if:`'s variables, so a dynamic `@skip(if: $flag)` resolves the same.
fn skipped(field: &async_graphql::SelectionField<'_>) -> bool {
    let Ok(directives) = field.directives() else {
        return false;
    };
    directives.iter().any(|directive| {
        let flag = directive
            .get_argument("if")
            .and_then(|argument| match &argument.node {
                async_graphql::Value::Boolean(flag) => Some(*flag),
                _ => None,
            });
        (directive.name.node.as_str() == "skip" && flag == Some(true))
            || (directive.name.node.as_str() == "include" && flag == Some(false))
    })
}

/// The schema's node type name for `slug`: letters, digits and `_` kept, the rest
/// `_`; a leading digit gains `_`; a reserved name gains `Entry`. Collection slugs
/// can't hold `_` — only hyphens, lowercased — so this is injective over collections.
fn type_name(slug: &str) -> String {
    let mut name: String = slug
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '_' {
                ch
            } else {
                '_'
            }
        })
        .collect();
    if name.starts_with(|ch: char| ch.is_ascii_digit()) {
        name.insert(0, '_');
    }
    if RESERVED_TYPES.contains(&name.as_str()) {
        name.push_str("Entry");
    }
    name
}

/// One collection's [`CollectionShape`]: the node's GraphQL name and each declared
/// field's, with its resolution. `readable` says which target collections expand —
/// the caller's `content.read`, either scope.
fn shape_of(
    collection: &str,
    definition: &Arc<ContentType>,
    readable: &dyn Fn(&str) -> bool,
) -> Arc<CollectionShape> {
    let declared: BTreeSet<String> = definition
        .fields()
        .iter()
        .map(|field| field.id.clone())
        .collect();
    let mut fields = BTreeMap::new();
    for field in definition.fields() {
        let resolve = match &field.kind {
            FieldKind::Number { .. } => Resolve::Number,
            FieldKind::Boolean => Resolve::Boolean,
            FieldKind::Json
            | FieldKind::RichText
            | FieldKind::Object { .. }
            | FieldKind::List { .. }
            | FieldKind::Blocks { .. } => Resolve::Json,
            FieldKind::Asset { many: true, .. } | FieldKind::Reference { many: true, .. } => {
                Resolve::IdList
            }
            FieldKind::Reference {
                to, many: false, ..
            } if readable(to) => Resolve::Reference { to: to.clone() },
            FieldKind::Reference { many: false, .. } => Resolve::Id,
            _ => Resolve::Text,
        };
        let name = gql_name(&field.id, &declared);
        fields.insert(
            name,
            GraphQLField {
                id: field.id.clone(),
                resolve,
            },
        );
    }
    Arc::new(CollectionShape {
        collection: collection.to_owned(),
        type_name: type_name(collection),
        fields,
        definition: definition.clone(),
    })
}

/// The [`Definitions`] a request's schema and resolvers share: one shape per readable
/// collection. `definitions` is collection → stored definition JSON; `collection`'s
/// own is already parsed.
fn defs_for(
    cache: &crate::type_cache::ContentTypeCache,
    definitions: &HashMap<String, String>,
    collection: &str,
    content_type: &Arc<ContentType>,
) -> Result<Arc<Definitions>, ApiError> {
    let readable = |slug: &str| definitions.contains_key(slug);
    let mut by_collection = HashMap::with_capacity(definitions.len());
    for (slug, definition) in definitions {
        let parsed = if slug == collection {
            content_type.clone()
        } else {
            cache.parse(definition)?
        };
        by_collection.insert(slug.clone(), shape_of(slug, &parsed, &readable));
    }
    Ok(Arc::new(Definitions { by_collection }))
}

/// A meta field's GraphQL type: one match keeps `NODE_META` and the schema in step.
fn meta_type(name: &str) -> TypeRef {
    match name {
        "id" | "createdBy" => TypeRef::named_nn(TypeRef::ID),
        "collection" => TypeRef::named_nn(TypeRef::STRING),
        "revisionId" => TypeRef::named(TypeRef::ID),
        "hiddenFields" => TypeRef::named_nn_list(TypeRef::STRING),
        "published" => TypeRef::named("Publication"),
        "content" => TypeRef::named("JSON"),
        // `title`, `locale`, `createdAt`, `updatedAt` — every other meta — is a
        // nullable string.
        _ => TypeRef::named(TypeRef::STRING),
    }
}

/// The object type a collection's nodes answer: the meta fields, then one field per
/// declared content field — its resolution by kind, captured per field.
fn node_object(shape: &Arc<CollectionShape>) -> Object {
    let mut object = Object::new(shape.type_name.clone())
        .description(format!("An entry of `{}`", shape.collection));
    for meta in NODE_META {
        let name = *meta;
        object = object.field(Field::new(name, meta_type(name), move |ctx| {
            FieldFuture::new(async move {
                let node = ctx
                    .parent_value
                    .try_downcast_ref::<Node>()
                    .map_err(internal)?;
                Ok(node_meta(node, name))
            })
        }));
    }
    for (name, field) in &shape.fields {
        let ty = field_type(&field.resolve);
        let field = field.clone();
        object = object.field(Field::new(name.clone(), ty, move |ctx| {
            let field = field.clone();
            FieldFuture::new(async move {
                let node = ctx
                    .parent_value
                    .try_downcast_ref::<Node>()
                    .map_err(internal)?;
                node_field(node, &field)
            })
        }));
    }
    object
}

/// A node's meta field's answer — `None` the `null` an unresolved reference or an
/// unpublished/missing member answers.
fn node_meta<'a>(node: &Node, meta: &str) -> Option<FieldValue<'a>> {
    let value = match meta {
        "id" => async_graphql::Value::from(node.id.clone()),
        "collection" => async_graphql::Value::from(node.collection.clone()),
        "revisionId" => node.revision_id.clone().map(async_graphql::Value::from)?,
        "title" => node.title.clone().map(async_graphql::Value::from)?,
        "hiddenFields" => async_graphql::Value::List(
            node.hidden_fields
                .clone()?
                .into_iter()
                .map(async_graphql::Value::from)
                .collect(),
        ),
        "published" => {
            return node.published.clone().map(FieldValue::owned_any);
        }
        "locale" => node.locale.clone().map(async_graphql::Value::from)?,
        "createdAt" => node.created_at.clone().map(async_graphql::Value::from)?,
        "updatedAt" => node.updated_at.clone().map(async_graphql::Value::from)?,
        "createdBy" => node.created_by.clone().map(async_graphql::Value::from)?,
        "content" => {
            let content = node.content.clone()?;
            async_graphql::Value::from_json(Value::Object(content)).ok()?
        }
        _ => return None,
    };
    Some(FieldValue::value(value))
}

/// The GraphQL type a content field answers: the scalar its kind writes, or the
/// target's node type for an expandable reference.
fn field_type(resolve: &Resolve) -> TypeRef {
    match resolve {
        Resolve::Text => TypeRef::named(TypeRef::STRING),
        Resolve::Number => TypeRef::named(TypeRef::FLOAT),
        Resolve::Boolean => TypeRef::named(TypeRef::BOOLEAN),
        Resolve::Json => TypeRef::named("JSON"),
        Resolve::Id => TypeRef::named(TypeRef::ID),
        Resolve::IdList => TypeRef::named_list_nn(TypeRef::STRING),
        Resolve::Reference { to } => TypeRef::named(type_name(to)),
    }
}

/// The `entries` resolver: arguments and selection → the canonical document →
/// [`entries::run_query`] → nodes. Aliased `entries` calls run this each time — the
/// `reads` counter bounds them.
async fn connection(ctx: ResolverContext<'_>) -> Result<Option<FieldValue<'_>>, Error> {
    let data = ctx.data::<Context>().map_err(internal)?;
    let reads = data.reads.fetch_add(1, Ordering::SeqCst) + 1;
    if reads > MAX_ROOT_READS {
        return Err(refused(Refusal {
            code: "too_many",
            pointer: String::new(),
            message: format!("a request runs at most {MAX_ROOT_READS} root reads"),
        }));
    }
    let shape = data
        .defs
        .by_collection
        .get(&data.collection)
        .ok_or_else(|| internal("the collection's shape is missing"))?;
    let arg = |name: &str| -> Result<Option<Value>, Error> {
        ctx.args
            .get(name)
            .map(|argument| argument.deserialize::<Value>())
            .transpose()
    };
    let filters = arg("where")?;
    let sort = arg("orderBy")?;
    let first = arg("first")?.and_then(|value| value.as_i64());
    let after = arg("after")?.and_then(|value| value.as_str().map(str::to_owned));
    // `nodes`' selections — fragments and aliases flattened by the lookahead — are the
    // node's own shape: fields keep, nested references deepen.
    let selected: Vec<Selected> = ctx
        .look_ahead()
        .field("nodes")
        .selection_fields()
        .iter()
        .flat_map(collect)
        .collect();
    let compiled = compile(
        &data.defs,
        shape,
        &selected,
        filters.as_ref().and_then(Value::as_object),
        sort.as_ref().and_then(Value::as_array),
        first,
    )
    .map_err(refused)?;
    let mut guard = data.tx.lock().await;
    let tx = guard
        .as_mut()
        .ok_or_else(|| internal("the transaction is gone"))?;
    let rows = entries::run_query(
        &data.state,
        tx,
        &data.collection,
        &data.content_type,
        &compiled.document,
        after.as_deref(),
    )
    .await
    .map_err(api_error)?;
    drop(guard);
    let nodes = rows
        .rows
        .into_iter()
        .map(node_of)
        .collect::<Result<Vec<_>, Error>>()?;
    Ok(Some(FieldValue::owned_any(Connection {
        nodes,
        end_cursor: rows.next_cursor.clone(),
        has_next: rows.next_cursor.is_some(),
        applied: Applied {
            query: serde_json::from_str(&rows.language.canonical_json()).map_err(internal)?,
            query_hash: entries::content_hash(&rows.language.query_hash()),
        },
    })))
}

/// The `entry(id:)` resolver: `get_with_live` + the caller's `Access` + the same
/// project/narrow/resolve/expand the page's rows get — an unreadable or absent entry
/// is `null`.
async fn one(ctx: ResolverContext<'_>) -> Result<Option<FieldValue<'_>>, Error> {
    let data = ctx.data::<Context>().map_err(internal)?;
    let reads = data.reads.fetch_add(1, Ordering::SeqCst) + 1;
    if reads > MAX_ROOT_READS {
        return Err(refused(Refusal {
            code: "too_many",
            pointer: String::new(),
            message: format!("a request runs at most {MAX_ROOT_READS} root reads"),
        }));
    }
    let Some(id) = ctx
        .args
        .get("id")
        .and_then(|argument| argument.string().ok().map(str::to_owned))
        .and_then(|text| Uuid::try_parse(&text).ok())
    else {
        // A malformed id names no entry; `entry` is nullable, so `null` like a miss.
        return Ok(None);
    };
    let shape = data
        .defs
        .by_collection
        .get(&data.collection)
        .ok_or_else(|| internal("the collection's shape is missing"))?;
    let selected: Vec<Selected> = ctx
        .look_ahead()
        .selection_fields()
        .iter()
        .flat_map(collect)
        .collect();
    let compiled = compile(&data.defs, shape, &selected, None, None, None).map_err(refused)?;
    let depth = compiled
        .document
        .get("depth")
        .and_then(Value::as_u64)
        .and_then(|value| u8::try_from(value).ok())
        .unwrap_or(0);
    if depth > colophon_domain::query::MAX_DEPTH {
        return Err(refused(Refusal {
            code: "out_of_range",
            pointer: "/depth".to_owned(),
            message: format!(
                "`depth` is a whole number from 0 to {}",
                colophon_domain::query::MAX_DEPTH
            ),
        }));
    }
    let mut guard = data.tx.lock().await;
    let tx = guard
        .as_mut()
        .ok_or_else(|| internal("the transaction is gone"))?;
    let Some((draft, live)) = db_entries::get_with_live(tx, &data.collection, id)
        .await
        .map_err(|error| api_error(ApiError::from_db(error)))?
    else {
        return Ok(None);
    };
    let access = Access::of(&tx.policy, tx.identity, &data.collection, draft.created_by);
    let Some(read) = access.read.as_ref() else {
        return Ok(None);
    };
    let (content, hidden_fields) = visibility::project(
        draft.content.clone(),
        read,
        &entries::field_ids(&data.content_type),
    )
    .map_err(api_error)?;
    let mut content = match &compiled.keep {
        Some(keep) => narrow(&content, keep).map_err(api_error)?,
        None => content,
    };
    if depth > 0 {
        let mut wanted = BTreeSet::new();
        crate::populate::collect_wanted(&data.content_type, &content, &mut wanted)
            .map_err(api_error)?;
        if !wanted.is_empty() {
            let resolved = crate::populate::resolve_drafts(&data.state, tx, &wanted, depth)
                .await
                .map_err(api_error)?;
            content = crate::populate::expand_document(&data.content_type, &content, &resolved)
                .map_err(api_error)?;
        }
    }
    drop(guard);
    let node = node_of(entries::QueryRow {
        title_visible: read.includes(data.content_type.title_field()),
        draft,
        live,
        content,
        hidden_fields,
    })?;
    Ok(Some(FieldValue::owned_any(node)))
}

/// A [`entries::QueryRow`] as a `Node`: the meta members filled from the draft,
/// `content` the (possibly expanded) document parsed back to an object — expansion
/// writes ids into objects, so the map is what resolvers see.
fn node_of(row: entries::QueryRow) -> Result<Node, Error> {
    let content = visibility::object(&row.content).map_err(api_error)?;
    Ok(Node {
        id: row.draft.entry_id.to_string(),
        collection: row.draft.collection.clone(),
        revision_id: Some(row.draft.revision.id.to_string()),
        title: Some(if row.title_visible {
            row.draft.revision.title.clone()
        } else {
            String::new()
        }),
        content: Some(content),
        hidden_fields: Some(row.hidden_fields),
        published: row.live.map(|(revision_id, generation)| Publication {
            revision_id: revision_id.to_string(),
            generation,
        }),
        locale: row.draft.locale.clone(),
        created_at: Some(entries::rfc3339(row.draft.created_at).map_err(api_error)?),
        updated_at: Some(entries::rfc3339(row.draft.updated_at).map_err(api_error)?),
        created_by: Some(row.draft.created_by.to_string()),
    })
}

/// The dynamic schema this request's definitions build: `Query` with `entries`
/// and `entry`, a node type per readable collection, the connection, `pageInfo`,
/// `applied` and `Publication` objects, and the queried collection's `where` /
/// `orderBy` inputs — which exist only while it declares an indexed field, an
/// input object or enum needing at least one member.
#[expect(
    clippy::too_many_lines,
    reason = "one schema's types and resolvers, emitted in one pass; splitting hides the shape"
)]
fn build_schema(context: Context) -> Result<Schema, ApiError> {
    let root = context
        .defs
        .by_collection
        .get(&context.collection)
        .ok_or(ApiError::NotFound)?
        .clone();
    let any_indexed = root.definition.fields().iter().any(|field| field.index);
    let connection_type = format!("{}Connection", root.type_name);
    let mut query = Object::new("Query");
    let mut entries_field = Field::new("entries", TypeRef::named(&connection_type), |ctx| {
        FieldFuture::new(connection(ctx))
    })
    .description("The bounded query language (D-103): drafts, paged.")
    .argument(InputValue::new("first", TypeRef::named(TypeRef::INT)))
    .argument(InputValue::new("after", TypeRef::named(TypeRef::STRING)));
    if any_indexed {
        entries_field = entries_field
            .argument(InputValue::new(
                "where",
                TypeRef::named(format!("{}Filter", root.type_name)),
            ))
            .argument(InputValue::new(
                "orderBy",
                TypeRef::named_nn_list(format!("{}Sort", root.type_name)),
            ));
    }
    query = query.field(entries_field).field(
        Field::new("entry", TypeRef::named(&root.type_name), |ctx| {
            FieldFuture::new(one(ctx))
        })
        .description("One draft by id; `null` where absent or unreadable.")
        .argument(InputValue::new("id", TypeRef::named_nn(TypeRef::ID))),
    );
    let defs = context.defs.clone();
    let mut builder = Schema::build("Query", None, None)
        .data(context)
        .register(Scalar::new("JSON"))
        .register(query)
        .register(
            Object::new("PageInfo")
                .field(Field::new(
                    "endCursor",
                    TypeRef::named(TypeRef::STRING),
                    |ctx| {
                        FieldFuture::new(async move {
                            let state = ctx
                                .parent_value
                                .try_downcast_ref::<PageState>()
                                .map_err(internal)?;
                            Ok(state.end_cursor.clone().map(FieldValue::value))
                        })
                    },
                ))
                .field(Field::new(
                    "hasNextPage",
                    TypeRef::named_nn(TypeRef::BOOLEAN),
                    |ctx| {
                        FieldFuture::new(async move {
                            let state = ctx
                                .parent_value
                                .try_downcast_ref::<PageState>()
                                .map_err(internal)?;
                            Ok(Some(FieldValue::value(state.has_next)))
                        })
                    },
                )),
        )
        .register(
            Object::new("AppliedQuery")
                .field(Field::new("query", TypeRef::named_nn("JSON"), |ctx| {
                    FieldFuture::new(async move {
                        let applied = ctx
                            .parent_value
                            .try_downcast_ref::<Applied>()
                            .map_err(internal)?;
                        Ok(Some(FieldValue::value(
                            async_graphql::Value::from_json(applied.query.clone())
                                .map_err(internal)?,
                        )))
                    })
                }))
                .field(Field::new(
                    "queryHash",
                    TypeRef::named_nn(TypeRef::STRING),
                    |ctx| {
                        FieldFuture::new(async move {
                            let applied = ctx
                                .parent_value
                                .try_downcast_ref::<Applied>()
                                .map_err(internal)?;
                            Ok(Some(FieldValue::value(applied.query_hash.clone())))
                        })
                    },
                )),
        )
        .register(
            Object::new("Publication")
                .field(Field::new(
                    "revisionId",
                    TypeRef::named_nn(TypeRef::ID),
                    |ctx| {
                        FieldFuture::new(async move {
                            let published = ctx
                                .parent_value
                                .try_downcast_ref::<Publication>()
                                .map_err(internal)?;
                            Ok(Some(FieldValue::value(published.revision_id.clone())))
                        })
                    },
                ))
                .field(Field::new(
                    "generation",
                    TypeRef::named_nn(TypeRef::FLOAT),
                    |ctx| {
                        FieldFuture::new(async move {
                            let published = ctx
                                .parent_value
                                .try_downcast_ref::<Publication>()
                                .map_err(internal)?;
                            // An integer-typed `ConstValue` under a `Float` field
                            // serializes as a JSON number either way, and loses
                            // nothing to `as f64` beyond i53.
                            Ok(Some(FieldValue::value(published.generation)))
                        })
                    },
                )),
        )
        .register(connection_object(&connection_type, &root.type_name));
    for shape in defs.by_collection.values() {
        builder = builder.register(node_object(shape));
    }
    if any_indexed {
        // `where`/`orderBy`/`dir`: inputs of the queried collection's indexed fields
        // only, the ops the kind takes — the document validator says the same
        // `field.index` says.
        let mut filter = InputObject::new(format!("{}Filter", root.type_name));
        let mut orderable = Enum::new(format!("{}Orderable", root.type_name));
        for (name, field) in &root.fields {
            let Some(declared) = root
                .definition
                .fields()
                .iter()
                .find(|declared| declared.id == field.id)
            else {
                continue;
            };
            if !declared.index {
                continue;
            }
            orderable = orderable.item(name.as_str());
            let ops_name = format!("{}Filter_{name}", root.type_name);
            builder = builder.register(ops_object(&ops_name, &declared.kind));
            filter = filter.field(InputValue::new(name.clone(), TypeRef::named(ops_name)));
        }
        builder = builder
            .register(filter)
            .register(orderable)
            .register(
                InputObject::new(format!("{}Sort", root.type_name))
                    .field(InputValue::new(
                        "field",
                        TypeRef::named_nn(format!("{}Orderable", root.type_name)),
                    ))
                    .field(
                        InputValue::new("dir", TypeRef::named("SortDirection"))
                            .default_value(async_graphql::Value::Enum(Name::new("ASC"))),
                    ),
            )
            .register(Enum::new("SortDirection").item("ASC").item("DESC"));
    }
    builder.finish().map_err(|error| {
        ApiError::internal(std::io::Error::other(format!(
            "the GraphQL schema could not be built: {error}"
        )))
    })
}

/// The `{T}Connection` object: `nodes: [T!]!`, `pageInfo: PageInfo!`, `applied`.
fn connection_object(connection: &str, node: &str) -> Object {
    Object::new(connection.to_owned())
        .field(Field::new(
            "nodes",
            TypeRef::named_nn_list_nn(node),
            |ctx| {
                FieldFuture::new(async move {
                    let connection = ctx
                        .parent_value
                        .try_downcast_ref::<Connection>()
                        .map_err(internal)?;
                    Ok(Some(FieldValue::list(
                        connection
                            .nodes
                            .iter()
                            .map(|node| FieldValue::owned_any(node.clone())),
                    )))
                })
            },
        ))
        .field(Field::new(
            "pageInfo",
            TypeRef::named_nn("PageInfo"),
            |ctx| {
                FieldFuture::new(async move {
                    let connection = ctx
                        .parent_value
                        .try_downcast_ref::<Connection>()
                        .map_err(internal)?;
                    Ok(Some(FieldValue::owned_any(PageState {
                        end_cursor: connection.end_cursor.clone(),
                        has_next: connection.has_next,
                    })))
                })
            },
        ))
        .field(Field::new(
            "applied",
            TypeRef::named_nn("AppliedQuery"),
            |ctx| {
                FieldFuture::new(async move {
                    let connection = ctx
                        .parent_value
                        .try_downcast_ref::<Connection>()
                        .map_err(internal)?;
                    Ok(Some(FieldValue::owned_any(Applied {
                        query: connection.applied.query.clone(),
                        query_hash: connection.applied.query_hash.clone(),
                    })))
                })
            },
        ))
}

/// The `{T}Filter_<field>` ops input: the ops the kind takes (the document's `ops_for`),
/// each the scalar the kind writes — `in` its list.
fn ops_object(name: &str, kind: &FieldKind) -> InputObject {
    let scalar = match kind {
        FieldKind::Number { .. } => TypeRef::FLOAT,
        FieldKind::Boolean => TypeRef::BOOLEAN,
        _ => TypeRef::STRING,
    };
    let ops: &[&str] = match kind {
        FieldKind::Text { .. } => &["eq", "in", "lt", "lte", "gt", "gte", "contains"],
        FieldKind::Number { .. }
        | FieldKind::DateTime
        | FieldKind::Date
        | FieldKind::Time
        | FieldKind::Slug { .. } => &["eq", "in", "lt", "lte", "gt", "gte"],
        _ => &["eq", "in"],
    };
    let mut object = InputObject::new(name.to_owned());
    for op in ops {
        let ty = if *op == "in" {
            TypeRef::named_nn_list(scalar)
        } else {
            TypeRef::named(scalar)
        };
        object = object.field(InputValue::new(*op, ty));
    }
    object
}

/// A [`Refusal`] as a GraphQL error: `message` its own, `extensions` holding the
/// `code` and `pointer` the API's `details` would.
fn refused(refusal: Refusal) -> Error {
    Error::new(refusal.message.clone()).extend_with(|_, extensions| {
        extensions.set("code", refusal.code);
        extensions.set("pointer", refusal.pointer);
    })
}

/// An [`ApiError`] inside a resolver — surfaced as a GraphQL error with the envelope's
/// `code`, so a refusal is refused the same way either API names it.
fn api_error(error: ApiError) -> Error {
    match error {
        ApiError::Invalid(refusal) => refused(refusal),
        other => {
            let public = other.public();
            Error::new(public.message).extend_with(|_, extensions| {
                extensions.set("code", public.code.as_str());
            })
        }
    }
}

/// An impossible-in-practice failure inside a resolver: the parent type is fixed by
/// the schema, so a wrong downcast is a bug, not a client error.
fn internal(error: impl std::fmt::Debug) -> Error {
    Error::new(format!("internal error: {error:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn posts_definition() -> Value {
        json!({
            "name": "Posts",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true, "index": true},
                {"id": "rating", "label": "Rating", "type": "number", "integer": true, "index": true},
                {"id": "section", "label": "Section", "type": "enum", "values": ["News", "Guide"], "index": true},
                {"id": "author", "label": "Author", "type": "reference", "to": "staff"},
                {"id": "related", "label": "Related", "type": "reference", "to": "posts", "many": true},
                {"id": "body", "label": "Body", "type": "rich_text"},
            ]
        })
    }

    fn staff_definition() -> Value {
        json!({
            "name": "Staff",
            "title_field": "name",
            "fields": [
                {"id": "name", "label": "Name", "type": "text", "required": true, "index": true},
                {"id": "mentor", "label": "Mentor", "type": "reference", "to": "staff"},
            ]
        })
    }

    fn selected(name: &str, children: Vec<Selected>) -> Selected {
        Selected {
            name: name.to_owned(),
            children,
        }
    }

    fn leaf(name: &str) -> Selected {
        selected(name, Vec::new())
    }

    fn defs(readable: &[&str]) -> Arc<Definitions> {
        let mut by_collection = HashMap::new();
        for slug in readable {
            let definition = match *slug {
                "posts" => posts_definition(),
                "staff" => staff_definition(),
                _ => unreachable!("a test definition"),
            };
            let parsed = ContentType::parse(&definition).expect("a valid definition");
            by_collection.insert(
                (*slug).to_owned(),
                shape_of(slug, &Arc::new(parsed), &|candidate| {
                    readable.contains(&candidate)
                }),
            );
        }
        Arc::new(Definitions { by_collection })
    }

    fn posts() -> (Arc<Definitions>, Arc<CollectionShape>) {
        let defs = defs(&["posts", "staff"]);
        (defs.clone(), defs.by_collection["posts"].clone())
    }

    fn language_of(shape: &CollectionShape, document: &Value) -> colophon_domain::query::Query {
        colophon_domain::query::Query::validate(shape.definition.as_ref(), document)
            .expect("the compiled document validates")
    }

    #[test]
    fn a_selection_compiles_to_the_same_document() {
        let (defs, shape) = posts();
        let compiled = compile(
            &defs,
            &shape,
            &[
                leaf("headline"),
                selected("author", vec![leaf("id"), leaf("collection")]),
            ],
            Some(
                json!({"section": {"eq": "News"}, "rating": {"gte": 3}})
                    .as_object()
                    .expect("object"),
            ),
            Some(&vec![json!({"field": "rating", "dir": "DESC"})]),
            Some(10),
        )
        .expect("a valid document");
        let language = language_of(&shape, &compiled.document);
        assert_eq!(language.limit(), 10);
        assert_eq!(language.depth(), 0);
        assert_eq!(
            language.fields(),
            Some(&["headline".to_owned(), "author".to_owned()][..])
        );
        assert_eq!(language.filters().len(), 2);
        assert_eq!(language.sort().len(), 1);
    }

    #[test]
    fn a_nested_reference_selection_is_depth() {
        let (defs, shape) = posts();
        let compiled = compile(
            &defs,
            &shape,
            &[selected("author", vec![leaf("name"), leaf("id")])],
            None,
            None,
            None,
        )
        .expect("compiled");
        assert_eq!(language_of(&shape, &compiled.document).depth(), 1);
    }

    #[test]
    fn two_levels_of_reference_is_depth_two() {
        let (defs, shape) = posts();
        let compiled = compile(
            &defs,
            &shape,
            &[selected(
                "author",
                vec![selected("mentor", vec![leaf("name")])],
            )],
            None,
            None,
            None,
        )
        .expect("compiled");
        assert_eq!(language_of(&shape, &compiled.document).depth(), 2);
    }

    #[test]
    fn a_third_level_refuses_like_the_document() {
        let (defs, shape) = posts();
        let compiled = compile(
            &defs,
            &shape,
            &[selected(
                "author",
                vec![selected(
                    "mentor",
                    vec![selected("mentor", vec![leaf("name")])],
                )],
            )],
            None,
            None,
            None,
        )
        .expect("compiled");
        let refusal =
            colophon_domain::query::Query::validate(&shape.definition, &compiled.document)
                .expect_err("depth 3 refuses");
        assert_eq!(refusal.code, "out_of_range");
        assert_eq!(refusal.pointer, "/depth");
    }

    #[test]
    fn an_unreadable_target_keeps_the_reference_an_id() {
        let defs = defs(&["posts"]);
        let shape = defs.by_collection["posts"].clone();
        assert!(matches!(shape.fields["author"].resolve, Resolve::Id));
        let compiled = compile(
            &defs,
            &shape,
            &[selected("author", vec![leaf("id")])],
            None,
            None,
            None,
        )
        .expect("compiled");
        assert_eq!(compiled.document["depth"], json!(0));
    }

    #[test]
    fn a_meta_named_field_is_renamed_not_dropped() {
        let definition = json!({
            "name": "Meta",
            "title_field": "headline",
            "fields": [
                {"id": "headline", "label": "Headline", "type": "text", "required": true},
                {"id": "id", "label": "Id", "type": "text"},
            ]
        });
        let parsed = ContentType::parse(&definition).expect("valid");
        let shape = shape_of("meta", &Arc::new(parsed), &|_| false);
        assert!(shape.fields.contains_key("field_id"));
        assert!(!shape.fields.contains_key("id"));
    }

    #[test]
    fn a_slug_is_a_valid_type_name() {
        assert_eq!(type_name("news-posts"), "news_posts");
        assert_eq!(type_name("query"), "queryEntry");
        assert_eq!(type_name("123abc"), "_123abc");
    }

    #[test]
    fn the_selection_bound_counts_aliases() {
        let selected: Vec<Selected> = (0..129).map(|_| leaf("headline")).collect();
        let defs = defs(&["posts"]);
        let shape = defs.by_collection["posts"].clone();
        let refusal = compile(&defs, &shape, &selected, None, None, None)
            .expect_err("too many selections refuse");
        assert_eq!(refusal.code, "too_many");
    }

    #[test]
    fn filters_and_sort_reuse_the_documents_bounds() {
        let (defs, shape) = posts();
        // Nine `where` entries compile to nine filters — the document refuses over
        // MAX_FILTERS just as the REST body does.
        let many: Map<String, Value> = (0..9)
            .map(|index| (format!("f{index}"), json!({"eq": 1})))
            .collect();
        let compiled = compile(&defs, &shape, &[], Some(&many), None, None)
            .expect("compile never refuses what the document would");
        // Meta-named `where` keys aren't fields — they're skipped, not sent onward.
        assert_eq!(compiled.document["filters"], Value::Null);
    }
}
