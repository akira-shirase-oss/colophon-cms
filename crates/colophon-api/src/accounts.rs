//! An administrator disables or enables an account (D-235): `POST /api/v1/accounts/{identity}/
//! disable` and `/enable`. Disabling revokes every session the account holds and every API token
//! it made, in the same transaction, and logs it; a disabled account then signs in like a wrong
//! password and gets no session by any route. Both answer 204 whether or not anything changed.

use axum::{
    extract::{Path, State},
    http::StatusCode,
};
use colophon_db::identity::{self, SetDisabled};
use serde::Deserialize;
use uuid::Uuid;

use crate::{
    ApiError, AppState,
    session::{SignedIn, services},
};

#[derive(Deserialize)]
pub(crate) struct AccountPath {
    identity: String,
}

async fn set(
    state: &AppState,
    caller: &SignedIn,
    path: &AccountPath,
    disable: bool,
) -> Result<StatusCode, ApiError> {
    let services = services(state)?;
    let target = Uuid::try_parse(&path.identity).map_err(|_| ApiError::NotFound)?;
    match identity::set_disabled(&services.pool, caller.identity, target, disable)
        .await
        .map_err(ApiError::from_db)?
    {
        SetDisabled::Done | SetDisabled::Unchanged => Ok(StatusCode::NO_CONTENT),
        SetDisabled::NotFound => Err(ApiError::NotFound),
        SetDisabled::NotAdministrator => Err(ApiError::forbidden(
            "administrators_only",
            "only an administrator can disable or enable an account",
        )),
        SetDisabled::OwnAccount => Err(ApiError::conflict(
            "own_account",
            "you can't disable your own account; another administrator can",
        )),
        SetDisabled::LastAdministrator => Err(ApiError::conflict(
            "last_administrator",
            "this is the last enabled administrator; make another one first",
        )),
    }
}

/// `POST /api/v1/accounts/{identity}/disable`.
pub(crate) async fn disable(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<AccountPath>,
) -> Result<StatusCode, ApiError> {
    set(&state, &caller, &path, true).await
}

/// `POST /api/v1/accounts/{identity}/enable`.
pub(crate) async fn enable(
    State(state): State<AppState>,
    caller: SignedIn,
    Path(path): Path<AccountPath>,
) -> Result<StatusCode, ApiError> {
    set(&state, &caller, &path, false).await
}
