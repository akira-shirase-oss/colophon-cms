//! An environment's languages (D-180): `GET {P}/locales` and `PUT {P}/locales`. The list is
//! replaced whole, in the body's order, under `If-Match` with its policy version's `ETag` and
//! an `Idempotency-Key`, by a role with `schema.change` — the same discipline as the review
//! stages (D-104) — and every change is audited. Delivery names the policy version in its cache
//! keys, so a change is never answered from a resolution cached under the old list.

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::Response,
};
use colophon_db::{
    audit,
    idempotency::{self, Claim},
    locales::{self, Replaced},
};
use colophon_domain::{
    locale::{self, Language},
    policy::Action,
    schema::Refusal,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    ApiError, AppState,
    entries::{idempotency_key, json, open, request_hash},
    error::{EntryInTheWay, InTheWay},
    members::require,
    session::SignedIn,
    shape::Data,
};

const UPDATE: &str = "locales.update";

#[derive(Deserialize)]
pub(crate) struct LocalesPath {
    project: String,
    environment: String,
}

/// One language, as the API reads and writes it.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct LanguageView {
    tag: String,
    label: String,
    #[serde(default)]
    default: bool,
    #[serde(default)]
    fallback: Vec<String>,
}

impl From<&Language> for LanguageView {
    fn from(language: &Language) -> Self {
        Self {
            tag: language.tag.clone(),
            label: language.label.clone(),
            default: language.default,
            fallback: language.fallback.clone(),
        }
    }
}

/// `PUT {P}/locales`'s body: the whole list, in display order.
#[derive(Deserialize)]
pub(crate) struct NewLocales {
    locales: Vec<LanguageView>,
}

/// The list and the version that names it.
#[derive(Serialize)]
struct LocalesView {
    version: i64,
    locales: Vec<LanguageView>,
}

/// The `ETag` of policy version `version`.
fn etag(version: i64) -> String {
    format!("\"locales-{version}\"")
}

fn respond(status: StatusCode, body: String, version: i64, replayed: bool) -> Response {
    let mut response = json(status, body, None, replayed);
    if let Ok(value) = HeaderValue::from_str(&etag(version)) {
        response.headers_mut().insert(header::ETAG, value);
    }
    response
}

/// The policy version `If-Match` names: `"locales-<n>"`.
fn if_match(headers: &HeaderMap) -> Result<i64, ApiError> {
    let value = headers
        .get(header::IF_MATCH)
        .ok_or(ApiError::PreconditionRequired(
            "changing languages needs an If-Match header with the ETag of the list you saw, so you change what you looked at",
        ))?;
    value
        .to_str()
        .ok()
        .and_then(|text| text.trim().strip_prefix("\"locales-")?.strip_suffix('"'))
        .and_then(|version| version.parse::<i64>().ok())
        .filter(|version| *version >= 0)
        .ok_or(ApiError::InvalidHeader(
            "If-Match must be one languages ETag, such as \"locales-3\"",
        ))
}

/// The canonical form a replace's request hash covers: one language a line, fields separated
/// by NUL (tags and labels can't hold one after the checks).
fn canonical(languages: &[Language]) -> String {
    languages.iter().fold(String::new(), |mut text, language| {
        text.push_str(&language.tag);
        text.push('\u{0}');
        text.push_str(&language.label);
        text.push('\u{0}');
        text.push_str(if language.default { "1" } else { "0" });
        for tag in &language.fallback {
            text.push('\u{0}');
            text.push_str(tag);
        }
        text.push('\n');
        text
    })
}

/// `GET {P}/locales`: the environment's languages in display order, with the policy version as
/// the `ETag` a `PUT` names. Any member may look; an empty list means one language and free tags.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<LocalesPath>,
) -> Result<Response, ApiError> {
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    let policy = locales::policy(&mut tx).await.map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    let text = serde_json::to_string(&Data {
        data: LocalesView {
            version: policy.version,
            locales: policy.languages.iter().map(LanguageView::from).collect(),
        },
    })
    .map_err(ApiError::internal)?;
    Ok(respond(StatusCode::OK, text, policy.version, false))
}

/// `PUT {P}/locales`: replaces the whole list in the body's order (D-180). Needs `If-Match`
/// with the list's `ETag`, an `Idempotency-Key` and `schema.change`. The list must have one
/// default (when it has any language) and fallbacks drawn from it, and a non-empty list must
/// cover every tag live entries carry (`locale_in_use`, naming them). Audited
/// `locales.replaced` with the lists before and after.
#[expect(
    clippy::too_many_lines,
    reason = "one write's own sequence: check, claim, compare, replace, audit, record"
)]
pub(crate) async fn replace(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<LocalesPath>,
    headers: HeaderMap,
    body: Result<Json<NewLocales>, JsonRejection>,
) -> Result<Response, ApiError> {
    let expected = if_match(&headers)?;
    let key = idempotency_key(&headers)?;
    let Json(body) = body?;
    let mut languages: Vec<Language> = body
        .locales
        .into_iter()
        .map(|view| Language {
            tag: view.tag,
            label: view.label,
            default: view.default,
            fallback: view.fallback,
        })
        .collect();
    locale::check_policy(&mut languages).map_err(|refusal| {
        ApiError::Invalid(Refusal {
            code: refusal.code,
            pointer: refusal.pointer,
            message: refusal.message,
        })
    })?;
    let mut tx = open(&state, caller, &path.project, &path.environment).await?;
    require(
        &tx.policy,
        Action::SchemaChange,
        None,
        "your role can't change this environment's languages",
    )?;
    let canonical = canonical(&languages);
    let request = request_hash(&[UPDATE, &expected.to_string(), &canonical]);
    match idempotency::claim(&mut tx, UPDATE, key, &request)
        .await
        .map_err(ApiError::from_db)?
    {
        Claim::Fresh => {}
        Claim::Mismatch => return Err(ApiError::IdempotencyKeyReused),
        Claim::Replay { status, response } => {
            let status = StatusCode::from_u16(status).map_err(ApiError::internal)?;
            let version = serde_json::from_str::<Value>(&response)
                .ok()
                .and_then(|body| body["data"]["version"].as_i64())
                .unwrap_or_default();
            return Ok(respond(status, response, version, true));
        }
    }
    let before = locales::policy(&mut tx).await.map_err(ApiError::from_db)?;
    let version = match locales::replace(&mut tx, expected, &languages)
        .await
        .map_err(ApiError::from_db)?
    {
        Replaced::Replaced { version } => version,
        Replaced::Stale { current } => return Err(ApiError::StaleLocales { current }),
        Replaced::InUse {
            tag,
            total,
            entries,
        } => {
            let noun = if total == 1 {
                "entry is"
            } else {
                "entries are"
            };
            return Err(ApiError::LocaleInUse(Box::new((
                tag.clone(),
                InTheWay {
                    reason: "locale_in_use",
                    pointer: "/locales".to_owned(),
                    message: format!(
                        "{total} {noun} written in {tag}. Move or delete them first, then remove the language"
                    ),
                    entries: entries
                        .into_iter()
                        .map(|entry| EntryInTheWay {
                            entry: entry.entry_id,
                            title: entry.title,
                            state: "draft",
                            problem: None,
                        })
                        .collect(),
                    total: u64::try_from(total).ok(),
                    content_type: None,
                },
            ))));
        }
    };
    let after: Vec<LanguageView> = languages.iter().map(LanguageView::from).collect();
    let detail = serde_json::json!({
        "environment": path.environment,
        "before": before.languages.iter().map(LanguageView::from).collect::<Vec<_>>(),
        "after": after,
        "version": version,
    });
    audit::record_in(
        &mut tx.transaction,
        tx.project,
        tx.identity,
        "locales.replaced",
        "locales",
        &detail.to_string(),
    )
    .await
    .map_err(ApiError::from_db)?;
    let text = serde_json::to_string(&Data {
        data: LocalesView {
            version,
            locales: after,
        },
    })
    .map_err(ApiError::internal)?;
    idempotency::complete(&mut tx, UPDATE, key, StatusCode::OK.as_u16(), &text)
        .await
        .map_err(ApiError::from_db)?;
    tx.commit().await.map_err(ApiError::from_db)?;
    Ok(respond(StatusCode::OK, text, version, false))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn headers(value: &str) -> HeaderMap {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::IF_MATCH,
            HeaderValue::from_str(value).expect("a header value"),
        );
        headers
    }

    #[test]
    fn if_match_names_one_policy_version() {
        assert_eq!(if_match(&headers("\"locales-0\"")).ok(), Some(0));
        assert_eq!(if_match(&headers(" \"locales-12\" ")).ok(), Some(12));
        for bad in [
            "*",
            "\"locales-\"",
            "locales-3",
            "\"locales--1\"",
            "\"sha256:ab\"",
        ] {
            assert!(if_match(&headers(bad)).is_err(), "{bad}");
        }
        assert!(matches!(
            if_match(&HeaderMap::new()),
            Err(ApiError::PreconditionRequired(_))
        ));
        assert_eq!(etag(4), "\"locales-4\"");
    }

    #[test]
    fn two_lists_hash_alike_only_when_they_are_alike() {
        let one = |label: &str, fallback: &[&str]| Language {
            tag: "pt".to_owned(),
            label: label.to_owned(),
            default: false,
            fallback: fallback.iter().map(|tag| (*tag).to_owned()).collect(),
        };
        assert_eq!(
            canonical(&[one("A", &["en"])]),
            canonical(&[one("A", &["en"])])
        );
        assert_ne!(canonical(&[one("A", &["en"])]), canonical(&[one("A", &[])]));
        assert_ne!(canonical(&[one("A", &[])]), canonical(&[one("B", &[])]));
    }
}
