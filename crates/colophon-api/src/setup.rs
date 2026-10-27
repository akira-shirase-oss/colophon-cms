//! `POST /api/v1/setup`: a signed-in person presents the setup code the server printed to its
//! terminal, and becomes the installation's first administrator (docs/modules/03-auth.md, "Owner
//! bootstrap"; D-060, amending D-043).
//!
//! Every property of the code is held here or below: the caller must be signed in (and, with a
//! cookie, send the CSRF token from a trusted origin); attempts are rate-limited in Postgres
//! before any comparison; the presented code is hashed at once and compared with every live hash
//! in constant time; only hashes reach the database; the code is used once, inside one
//! transaction that also makes the administrator; and once anyone is an administrator, every
//! step refuses. The code never appears in a response, and nothing here logs it.

use axum::{
    Json,
    extract::{State, rejection::JsonRejection},
};
use colophon_auth::tokens;
use colophon_db::setup::{self, Attempt, Claimed, LiveCode};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    session::{SignedIn, services},
    shape::Data,
};

/// What the caller presents. `Debug` is not derived: the code must never be printed.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Presented {
    code: String,
}

#[derive(Serialize)]
pub(crate) struct SetUp {
    /// Always true: the caller is now an administrator.
    administrator: bool,
}

/// What a wrong, used or expired code gets. One message for all three: telling them apart would
/// tell a guesser which codes once existed.
fn invalid() -> ApiError {
    ApiError::InvalidField {
        reason: "setup_code_invalid",
        pointer: "/code",
        message: "that setup code isn't right, or it was used or has expired: copy the one the server printed when it last started, or restart the server for a new one".to_owned(),
    }
}

/// The live code whose hash is `presented`, if any. Every live hash is compared, in constant
/// time, whatever matches first: how long this takes says nothing about which code is close.
pub(crate) fn matching(presented: &[u8; 32], live: &[LiveCode]) -> Option<Uuid> {
    let mut found = None;
    for code in live {
        if tokens::same_secret(presented, &code.hash) && found.is_none() {
            found = Some(code.id);
        }
    }
    found
}

/// `POST /api/v1/setup` with `{"code": "…"}`.
pub(crate) async fn claim(
    State(state): State<AppState>,
    caller: SignedIn,
    body: Result<Json<Presented>, JsonRejection>,
) -> Result<Json<Data<SetUp>>, ApiError> {
    let services = services(&state)?;
    let Json(Presented { code }) = body?;
    // Hash first, and let the plain code go: from here on only the hash exists.
    let presented = tokens::setup_code_hash(&code);
    drop(code);
    match setup::attempt(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?
    {
        Attempt::Allowed => {}
        Attempt::AlreadySetUp => return Err(ApiError::AlreadySetUp),
        Attempt::Limited { retry_after } => return Err(ApiError::RateLimited { retry_after }),
    }
    let presented = presented.ok_or_else(invalid)?;
    let live = setup::live_codes(&services.pool)
        .await
        .map_err(ApiError::from_db)?;
    let Some(code) = matching(&presented, &live) else {
        return Err(invalid());
    };
    match setup::claim(&services.pool, caller.identity, code, &presented)
        .await
        .map_err(ApiError::from_db)?
    {
        Claimed::Claimed => {
            tracing::info!(identity = %caller.identity, "setup code used: first administrator made");
            Ok(Json(Data {
                data: SetUp {
                    administrator: true,
                },
            }))
        }
        Claimed::Invalid => Err(invalid()),
        Claimed::AlreadySetUp => Err(ApiError::AlreadySetUp),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn code(id: u128, text: &str) -> LiveCode {
        LiveCode {
            id: Uuid::from_u128(id),
            hash: tokens::hash(text),
        }
    }

    #[test]
    fn only_an_exact_hash_matches_and_the_first_one_wins() {
        let live = [code(1, "AAAA"), code(2, "BBBB"), code(3, "BBBB")];
        assert_eq!(
            matching(&tokens::hash("BBBB"), &live),
            Some(Uuid::from_u128(2))
        );
        assert_eq!(matching(&tokens::hash("CCCC"), &live), None);
        assert_eq!(matching(&tokens::hash("AAAA"), &[]), None);
        // One flipped bit is a different code.
        let mut near = tokens::hash("AAAA");
        near[31] ^= 1;
        assert_eq!(matching(&near, &live), None);
    }

    #[test]
    fn the_invalid_message_never_says_which_way_a_code_is_wrong() {
        let message = invalid().to_string();
        assert!(message.contains("isn't right, or it was used or has expired"));
        assert_eq!(
            invalid().public().details.map(|d| d.reason),
            Some("setup_code_invalid")
        );
    }
}
