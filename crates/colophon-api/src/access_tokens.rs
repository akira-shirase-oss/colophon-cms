//! `GET`/`POST`/`DELETE /api/v1/tokens`: the caller's access tokens. A token is shown in
//! full exactly once, when it is created; afterwards only its name and dates are listed.
//! Tokens are identity-scoped, not project-scoped, so there is no project transaction and
//! no `Idempotency-Key`: a replayed create mints a second, named token.

use std::time::Duration;

use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
    response::{IntoResponse, Response},
};
use colophon_auth::tokens;
use colophon_db::access::{self, TokenInfo};
use colophon_domain::schema::Refusal;
use serde::{Deserialize, Serialize};
use serde_json::json;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    session::{SignedIn, services},
    shape::Data,
};

/// A token named in the Studio lasts as long as one from device sign-in: thirty days.
const TOKEN_LIFETIME: Duration = Duration::from_hours(30 * 24);

/// A token as the list shows it — its plaintext is never stored, so it can't be listed.
#[derive(Serialize)]
pub(crate) struct TokenView {
    id: Uuid,
    name: String,
    created_at: String,
    last_used_at: String,
    expires_at: String,
}

fn view(token: &TokenInfo) -> Result<TokenView, ApiError> {
    Ok(TokenView {
        id: token.id,
        name: token.name.clone(),
        created_at: crate::entries::rfc3339(token.created_at)?,
        last_used_at: crate::entries::rfc3339(token.last_used_at)?,
        expires_at: crate::entries::rfc3339(token.expires_at)?,
    })
}

/// `GET /api/v1/tokens`: the caller's live tokens, newest first.
pub(crate) async fn list(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Json<Data<Vec<TokenView>>>, ApiError> {
    let services = services(&state)?;
    let tokens = access::list_access_tokens(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?;
    let data = tokens
        .iter()
        .map(view)
        .collect::<Result<Vec<TokenView>, ApiError>>()?;
    Ok(Json(Data { data }))
}

/// The body of `POST /api/v1/tokens`.
#[derive(Deserialize)]
pub(crate) struct Named {
    name: String,
}

/// `POST /api/v1/tokens`: mints a named token; its plaintext is in this response only.
pub(crate) async fn create(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<Named>, JsonRejection>,
) -> Result<Response, ApiError> {
    let Json(named) = body?;
    let name = named.name.trim();
    if name.is_empty() || name.chars().count() > 100 {
        return Err(ApiError::Invalid(Refusal {
            code: "invalid_name",
            pointer: "/name".to_owned(),
            message: "a token needs a name of at most 100 characters".to_owned(),
        }));
    }
    let services = services(&state)?;
    let token = tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
    let expires_at = OffsetDateTime::now_utc() + TOKEN_LIFETIME;
    let id = access::create_access_token(
        &services.pool,
        caller.identity,
        &tokens::hash(&token),
        name,
        expires_at,
    )
    .await
    .map_err(ApiError::from_db)?;
    tracing::info!(%caller.identity, "access token issued");
    Ok((
        StatusCode::CREATED,
        Json(Data {
            data: json!({
                "id": id,
                "token": token,
                "name": name,
                "expires_at": crate::entries::rfc3339(expires_at)?,
            }),
        }),
    )
        .into_response())
}

/// `DELETE /api/v1/tokens/{token}`: revokes the caller's own token; another person's is 404.
pub(crate) async fn revoke(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(token): Path<String>,
) -> Result<Response, ApiError> {
    let id = Uuid::try_parse(&token).map_err(|_| ApiError::NotFound)?;
    let services = services(&state)?;
    if access::revoke_own_access_token(&services.pool, caller.identity, id)
        .await
        .map_err(ApiError::from_db)?
    {
        Ok(Json(Data {
            data: json!({ "removed": true }),
        })
        .into_response())
    } else {
        Err(ApiError::NotFound)
    }
}
