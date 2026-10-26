//! Signing in from a terminal (docs/modules/03-auth.md; D-041), in the shape of the OAuth device
//! authorization grant (RFC 8628), approved through Colophon's own browser session:
//!
//! 1. `POST /auth/device` (no sign-in): the client gets a secret to poll with and a short code;
//! 2. the person opens `/studio/device?code=…` where they are signed in, sees what is asking
//!    (`GET /auth/device/{code}`), and approves it (`POST /auth/device/approve`);
//! 3. `POST /auth/device/token` with the secret returns "pending" until then, and afterwards a
//!    personal access token, exactly once.
//!
//! Codes live 10 minutes; tokens 30 days. Only hashes are stored. Only a browser session may
//! approve, so a token can't mint more tokens.

use std::time::Duration;

use aws_lc_rs::rand::{SecureRandom as _, SystemRandom};
use axum::{
    Json,
    extract::{Path, State, rejection::JsonRejection},
    http::StatusCode,
};
use colophon_auth::tokens;
use colophon_db::{DbError, access};
use serde::{Deserialize, Serialize};
use time::OffsetDateTime;

use crate::{
    ApiError, AppState,
    session::{Credential, SignedIn, services},
    shape::Data,
};

/// How long a device code works.
const DEVICE_WINDOW: Duration = Duration::from_mins(10);
/// How long an access token works.
const TOKEN_LIFETIME: Duration = Duration::from_hours(30 * 24);
/// Seconds a client should wait between polls.
const POLL_INTERVAL: u64 = 2;
/// User-code letters: no vowels (no accidental words) and nothing that looks alike (0/O, 1/I).
const ALPHABET: &[u8; 28] = b"BCDFGHJKLMNPQRSTVWXZ23456789";

fn random() -> Result<String, ApiError> {
    tokens::random_token()
        .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))
}

/// `XXXX-XXXX` from [`ALPHABET`], each letter equally likely (rejection sampling).
fn user_code() -> Result<String, ApiError> {
    let rng = SystemRandom::new();
    let mut code = String::with_capacity(9);
    while code.len() < 9 {
        if code.len() == 4 {
            code.push('-');
            continue;
        }
        let mut byte = [0_u8; 1];
        rng.fill(&mut byte)
            .map_err(|_| ApiError::internal(std::io::Error::other("no secure random source")))?;
        // 252 = 9 × 28: anything above would make the first letters more likely.
        if byte[0] < 252 {
            code.push(char::from(ALPHABET[usize::from(byte[0]) % ALPHABET.len()]));
        }
    }
    Ok(code)
}

fn expires_in(duration: Duration) -> OffsetDateTime {
    OffsetDateTime::now_utc() + time::Duration::try_from(duration).unwrap_or(time::Duration::HOUR)
}

/// `{"client_name": "..."}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Start {
    client_name: String,
}

#[derive(Serialize)]
pub(crate) struct Started {
    device_secret: String,
    user_code: String,
    verification_uri: String,
    interval: u64,
    expires_in: u64,
}

/// `POST /auth/device`.
pub(crate) async fn start(
    State(state): State<AppState>,
    body: Result<Json<Start>, JsonRejection>,
) -> Result<(StatusCode, Json<Data<Started>>), ApiError> {
    let services = services(&state)?;
    let Json(body) = body?;
    let name = body.client_name.trim();
    if !(1..=100).contains(&name.chars().count()) || name.chars().any(char::is_control) {
        return Err(ApiError::InvalidField {
            reason: "invalid_client_name",
            pointer: "/client_name",
            message: "a client name is 1 to 100 characters, without control characters".to_owned(),
        });
    }
    let secret = random()?;
    // A clash between two live codes is possible, if unlikely: draw again.
    for _ in 0..3 {
        let code = user_code()?;
        match access::start_device_login(
            &services.pool,
            &tokens::hash(&secret),
            &code,
            name,
            expires_in(DEVICE_WINDOW),
        )
        .await
        {
            Ok(()) => {
                let origin = services
                    .trusted_origins
                    .first()
                    .cloned()
                    .unwrap_or_default();
                return Ok((
                    StatusCode::CREATED,
                    Json(Data {
                        data: Started {
                            device_secret: secret,
                            verification_uri: format!("{origin}/studio/device?code={code}"),
                            user_code: code,
                            interval: POLL_INTERVAL,
                            expires_in: DEVICE_WINDOW.as_secs(),
                        },
                    }),
                ));
            }
            Err(DbError::Conflict) => {}
            Err(other) => return Err(ApiError::from_db(other)),
        }
    }
    Err(ApiError::Busy)
}

#[derive(Serialize)]
pub(crate) struct Pending {
    client_name: String,
    expires_at: String,
    approved: bool,
}

/// `GET /auth/device/{code}`: what is asking, for the approval page.
pub(crate) async fn show(
    State(state): State<AppState>,
    // Only someone signed in may see what is asking to sign in.
    _caller: SignedIn,
    Path(code): Path<String>,
) -> Result<Json<Data<Pending>>, ApiError> {
    let services = services(&state)?;
    let login = access::device_login(&services.pool, &code.to_ascii_uppercase())
        .await
        .map_err(ApiError::from_db)?
        .ok_or(ApiError::NotFound)?;
    Ok(Json(Data {
        data: Pending {
            client_name: login.client_name,
            expires_at: crate::entries::rfc3339(login.expires_at)?,
            approved: login.approved,
        },
    }))
}

/// `{"user_code": "XXXX-XXXX"}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Approve {
    user_code: String,
}

/// `POST /auth/device/approve`: lets the device sign in as the caller.
pub(crate) async fn approve(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<Approve>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let services = services(&state)?;
    if caller.credential != Credential::Session {
        return Err(ApiError::Forbidden(
            "approve a device from a browser where you are signed in",
        ));
    }
    let Json(body) = body?;
    if access::approve_device_login(
        &services.pool,
        &body.user_code.to_ascii_uppercase(),
        caller.identity,
    )
    .await
    .map_err(ApiError::from_db)?
    {
        tracing::info!(identity = %caller.identity, "device sign-in approved");
        Ok(StatusCode::NO_CONTENT)
    } else {
        Err(ApiError::NotFound)
    }
}

/// `{"device_secret": "..."}`.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Poll {
    device_secret: String,
}

#[derive(Serialize)]
pub(crate) struct Polled {
    status: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    access_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    expires_at: Option<String>,
}

/// `POST /auth/device/token`: "pending", or the access token, once.
pub(crate) async fn token(
    State(state): State<AppState>,
    body: Result<Json<Poll>, JsonRejection>,
) -> Result<Json<Data<Polled>>, ApiError> {
    let services = services(&state)?;
    let Json(body) = body?;
    if body.device_secret.len() != 43 {
        return Err(ApiError::NotFound);
    }
    match access::poll_device_login(&services.pool, &tokens::hash(&body.device_secret))
        .await
        .map_err(ApiError::from_db)?
    {
        access::Poll::Pending => Ok(Json(Data {
            data: Polled {
                status: "pending",
                access_token: None,
                expires_at: None,
            },
        })),
        access::Poll::Approved {
            identity,
            client_name,
        } => {
            let token = random()?;
            let expires_at = expires_in(TOKEN_LIFETIME);
            access::create_access_token(
                &services.pool,
                identity,
                &tokens::hash(&token),
                &client_name,
                expires_at,
            )
            .await
            .map_err(ApiError::from_db)?;
            tracing::info!(%identity, "access token issued");
            Ok(Json(Data {
                data: Polled {
                    status: "approved",
                    access_token: Some(token),
                    expires_at: Some(crate::entries::rfc3339(expires_at)?),
                },
            }))
        }
        access::Poll::Gone => Err(ApiError::NotFound),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_codes_are_two_groups_of_four_unambiguous_letters() {
        for _ in 0..200 {
            let code = user_code().expect("random");
            assert_eq!(code.len(), 9);
            assert_eq!(code.as_bytes()[4], b'-');
            assert!(
                code.bytes()
                    .enumerate()
                    .all(|(i, b)| i == 4 || ALPHABET.contains(&b)),
                "{code}"
            );
        }
    }
}
