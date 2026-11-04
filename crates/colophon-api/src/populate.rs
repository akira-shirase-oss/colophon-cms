//! The query language's `depth`, applied (DX.5, D-107): a page's reference fields name
//! entry ids; this resolves them level by level — one batched `IN` read per level, at
//! most [`colophon_domain::query::MAX_DEPTH`] of them, never one read per row or field —
//! and rewrites each returned document's reference values into the objects that name
//! their entries ([`colophon_domain::populate`] for the rewrite itself).
//!
//! Each perspective resolves against its own read:
//!
//! - the draft query — `POST …/entries/query` — resolves each level's current drafts
//!   in the caller's transaction, then shows of each target only what the caller's own
//!   `content.read` allows on the target's collection, `own` condition included: an
//!   entry the role can't read keeps its stored id, and one it can read expands with
//!   its fields narrowed and `hidden_fields` reported like a draft view's;
//! - the delivery query — `POST /content/v1/…/query` — resolves each level's
//!   publications pinned at the page's own generation, so a populated reference reads
//!   exactly the release its row does; everything published is public, so no
//!   narrowing applies — and a publication revoked or taken down since doesn't
//!   resolve at any generation, staying the stored id.
//!
//! A cycle can never run away: each level's targets expand only through the level
//! below's resolutions, and there is no level past the bound.

use std::{
    collections::{BTreeSet, HashMap},
    sync::Arc,
};

use colophon_db::{
    content_types, delivery::DeliveryTransaction, query as db_query, scope::ProjectTransaction,
};
use colophon_domain::{
    policy::Action,
    populate::{self, Expanded},
    schema::ContentType,
};
use serde_json::{Map, Value};
use uuid::Uuid;

use crate::{
    ApiError, AppState, Services,
    entries::{content_type, field_ids},
    visibility,
};

/// The map one level's ids resolve to: the stored id's spelling to its [`Expanded`].
pub(crate) type Resolved = HashMap<String, Expanded>;

/// One fetched expansion target, narrowed and ready to embed: the level's [`Resolved`]
/// is assembled out of these once the level below's exists.
struct Staged {
    /// The target's entry.
    id: Uuid,
    /// Its collection.
    collection: String,
    /// The revision the resolution read.
    revision_id: Uuid,
    /// Its title, already blanked when the caller may not read it.
    title: String,
    /// Fields of its type the caller may not read (the draft perspective; always
    /// empty on the published one).
    hidden_fields: Vec<String>,
    /// Its collection's content type — needed to expand this level's own reference
    /// fields with the level below's resolutions; `None` when it couldn't be read
    /// and this level is the deepest anyway.
    definition: Option<Arc<ContentType>>,
    /// Its document as it will embed: narrowed, still holding its own reference ids.
    document: Map<String, Value>,
}

/// The entry ids `content`'s top-level single-reference fields name, added to `into`:
/// what one expansion level batch-resolves ([`crate::populate::collect_ids`] over the
/// document, the strings it yields parsed as the uuids stored references always are).
pub(crate) fn collect_wanted(
    definition: &ContentType,
    content: &str,
    into: &mut BTreeSet<Uuid>,
) -> Result<(), ApiError> {
    let document = visibility::object(content)?;
    let mut named = BTreeSet::new();
    populate::collect_ids(definition, &document, &mut named);
    for id in named {
        // Stored references validate as uuids; anything else simply can't resolve.
        if let Ok(id) = Uuid::try_parse(&id) {
            into.insert(id);
        }
    }
    Ok(())
}

/// `content` with its single-reference values replaced by their resolved objects:
/// the expansion a `depth` ≥ 1 answer applies to each row, on either perspective.
/// `resolved` empty means nothing could resolve — `content` is returned untouched.
pub(crate) fn expand_document(
    definition: &ContentType,
    content: &str,
    resolved: &Resolved,
) -> Result<String, ApiError> {
    if resolved.is_empty() {
        return Ok(content.to_owned());
    }
    let mut document = visibility::object(content)?;
    populate::expand(definition, &mut document, resolved);
    serde_json::to_string(&Value::Object(document)).map_err(ApiError::internal)
}

/// The collection's current content type, cached for the resolution: `None` when the
/// collection is gone — a race a target survives as its stored id.
async fn draft_definition(
    state: &AppState,
    tx: &mut ProjectTransaction,
    definitions: &mut HashMap<String, Arc<ContentType>>,
    collection: &str,
) -> Result<Option<Arc<ContentType>>, ApiError> {
    if !definitions.contains_key(collection) {
        match content_type(state, tx, collection).await {
            Ok((parsed, _)) => {
                definitions.insert(collection.to_owned(), parsed);
            }
            Err(ApiError::NotFound) => return Ok(None),
            Err(error) => return Err(error),
        }
    }
    Ok(definitions.get(collection).cloned())
}

/// The same, for the published perspective: the definition lives outside the
/// delivery role's reach, so it's read as the API role in the project's context,
/// like the query document's own validation does (DX.4, D-105).
async fn published_definition(
    services: &Services,
    project: Uuid,
    environment: Uuid,
    definitions: &mut HashMap<String, Arc<ContentType>>,
    collection: &str,
) -> Result<Option<Arc<ContentType>>, ApiError> {
    if !definitions.contains_key(collection) {
        let Some(definition) =
            content_types::current_definition(&services.pool, project, environment, collection)
                .await
                .map_err(ApiError::from_db)?
        else {
            return Ok(None);
        };
        definitions.insert(
            collection.to_owned(),
            services.content_types.parse(&definition)?,
        );
    }
    Ok(definitions.get(collection).cloned())
}

/// The ids `wanted` names, resolved `depth` levels deep against the draft
/// perspective: each level's current drafts fetched in one `ANY` inside the caller's
/// transaction — one snapshot — then narrowed to the caller's `content.read` fields
/// on the target's collection, `own` condition included. A target the role can't
/// read, or one that's gone, resolves to nothing: its reference stays the stored id.
pub(crate) async fn resolve_drafts(
    state: &AppState,
    tx: &mut ProjectTransaction,
    wanted: &BTreeSet<Uuid>,
    depth: u8,
) -> Result<Resolved, ApiError> {
    let mut definitions: HashMap<String, Arc<ContentType>> = HashMap::new();
    let mut frames: Vec<Vec<Staged>> = Vec::new();
    let mut ids: Vec<Uuid> = wanted.iter().copied().collect();
    for level in 0..depth {
        if ids.is_empty() {
            break;
        }
        let targets = db_query::resolved_drafts(tx, &ids)
            .await
            .map_err(ApiError::from_db)?;
        let mut below = BTreeSet::new();
        let mut frame = Vec::with_capacity(targets.len());
        for target in targets {
            // Only what a direct read would show: a target the role can't read on
            // this entry keeps the reference's stored id.
            let Some(read) = tx.policy.fields(
                Action::ContentRead,
                &target.collection,
                target.created_by == tx.identity,
            ) else {
                continue;
            };
            // The target's own definition names its fields and its reference fields;
            // a collection gone mid-read leaves the id unexpanded.
            let Some(definition) =
                draft_definition(state, tx, &mut definitions, &target.collection).await?
            else {
                continue;
            };
            let (projected, hidden_fields) =
                visibility::project(target.content.clone(), &read, &field_ids(&definition))?;
            let document = visibility::object(&projected)?;
            if level + 1 < depth {
                collect_wanted(&definition, &projected, &mut below)?;
            }
            let title = if read.includes(definition.title_field()) {
                target.revision.title.clone()
            } else {
                String::new()
            };
            frame.push(Staged {
                id: target.entry_id,
                collection: target.collection,
                revision_id: target.revision.id,
                title,
                hidden_fields,
                definition: Some(definition),
                document,
            });
        }
        frames.push(frame);
        ids = below.into_iter().collect();
    }
    Ok(assemble(frames))
}

/// The same, against the published perspective: each level's publications pinned at
/// `generation` — the pin the page's own rows answer under — fetched inside the
/// delivery transaction. Nothing narrows: published content is public. A publication
/// revoked or unpublished since resolves to nothing, at any generation.
pub(crate) async fn resolve_publications(
    services: &Services,
    tx: &mut DeliveryTransaction,
    project: Uuid,
    environment: Uuid,
    wanted: &BTreeSet<Uuid>,
    generation: i64,
    depth: u8,
) -> Result<Resolved, ApiError> {
    let mut definitions: HashMap<String, Arc<ContentType>> = HashMap::new();
    let mut frames: Vec<Vec<Staged>> = Vec::new();
    let mut ids: Vec<Uuid> = wanted.iter().copied().collect();
    for level in 0..depth {
        if ids.is_empty() {
            break;
        }
        let targets = db_query::resolved_publications(tx, environment, &ids, generation)
            .await
            .map_err(ApiError::from_db)?;
        let mut below = BTreeSet::new();
        let mut frame = Vec::with_capacity(targets.len());
        for target in targets {
            // Only while the level below still expands is the target's own definition
            // needed — to name its reference fields. It stays `None` at the bound.
            let definition = if level + 1 < depth {
                published_definition(
                    services,
                    project,
                    environment,
                    &mut definitions,
                    &target.collection,
                )
                .await?
            } else {
                None
            };
            let document = visibility::object(&target.content)?;
            if let Some(definition) = &definition {
                let mut named = BTreeSet::new();
                populate::collect_ids(definition, &document, &mut named);
                for id in named {
                    if let Ok(id) = Uuid::try_parse(&id) {
                        below.insert(id);
                    }
                }
            }
            frame.push(Staged {
                id: target.entry_id,
                collection: target.collection.clone(),
                revision_id: target.revision_id,
                title: target.title,
                hidden_fields: Vec::new(),
                definition,
                document,
            });
        }
        frames.push(frame);
        ids = below.into_iter().collect();
    }
    Ok(assemble(frames))
}

/// The levels' [`Resolved`] map, assembled from the deepest up: each target's
/// embedded document is expanded with the level below's resolutions — empty at the
/// bound, so the deepest targets' own references stay the stored ids — then keyed by
/// the stored id's spelling for the level above's own expansion.
fn assemble(frames: Vec<Vec<Staged>>) -> Resolved {
    let mut below: Resolved = HashMap::new();
    for frame in frames.into_iter().rev() {
        let mut resolved = Resolved::with_capacity(frame.len());
        for mut staged in frame {
            if let Some(definition) = &staged.definition {
                populate::expand(definition, &mut staged.document, &below);
            }
            resolved.insert(
                staged.id.to_string(),
                Expanded {
                    collection: staged.collection,
                    revision_id: staged.revision_id.to_string(),
                    title: staged.title,
                    content: Value::Object(staged.document),
                    hidden_fields: staged.hidden_fields,
                },
            );
        }
        below = resolved;
    }
    below
}
