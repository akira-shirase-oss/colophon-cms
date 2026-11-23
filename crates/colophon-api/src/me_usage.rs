//! `GET /api/v1/me/usage` (D-235): what the signed-in account uses against its limits, across
//! the projects it owns. Read through `colophon_account_usage()`, which answers the caller's own
//! row only; a `null` limit is unbounded (an administrator's, or a quota configured as 0).

use axum::{Json, extract::State};
use serde::Serialize;

use crate::{
    ApiError, AppState,
    session::{SignedIn, services},
    shape::Data,
};

/// One quota: what is used and its limit, `None` when unbounded.
#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Meter {
    used: u64,
    limit: Option<u64>,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
pub(crate) struct Usage {
    projects: Meter,
    entries: Meter,
    assets: Meter,
    storage_bytes: Meter,
}

/// The project cap a non-administrator gets: the per-account cap where registration lets
/// members create projects, otherwise none at all (only administrators create them).
fn project_limit(exempt: bool, self_serve: bool, cap: u32) -> Option<u64> {
    match (exempt, self_serve) {
        (true, _) => None,
        (false, true) => Some(u64::from(cap)),
        (false, false) => Some(0),
    }
}

fn usage_of(held: &colophon_db::usage::Usage, self_serve: bool, cap: u32) -> Usage {
    // A quota of 0 is configured as no bound; an administrator is never refused.
    let bound = |limit: u64| (!held.exempt && limit > 0).then_some(limit);
    Usage {
        projects: Meter {
            used: held.projects,
            limit: project_limit(held.exempt, self_serve, cap),
        },
        entries: Meter {
            used: held.entries,
            limit: bound(held.limits.entries),
        },
        assets: Meter {
            used: held.assets,
            limit: bound(held.limits.assets),
        },
        // Quarantined uploads count against storage as the triggers count them.
        storage_bytes: Meter {
            used: held.stored_bytes.saturating_add(held.pending_bytes),
            limit: bound(held.limits.storage_bytes),
        },
    }
}

/// `GET /api/v1/me/usage`.
pub(crate) async fn usage(
    State(state): State<AppState>,
    caller: SignedIn,
) -> Result<Json<Data<Usage>>, ApiError> {
    let services = services(&state)?;
    let held = colophon_db::usage::of_identity(&services.pool, caller.identity)
        .await
        .map_err(ApiError::from_db)?;
    Ok(Json(Data {
        data: usage_of(
            &held,
            services.selfserve.registration.self_serve_projects(),
            services.selfserve.projects_per_account,
        ),
    }))
}

#[cfg(test)]
mod tests {
    use colophon_db::usage::{Limits, Usage as Held};

    use super::*;

    fn held(exempt: bool, limits: Limits) -> Held {
        Held {
            entries: 7,
            assets: 2,
            stored_bytes: 1_000,
            pending_bytes: 24,
            limits,
            exempt,
            projects: 1,
        }
    }

    const LIMITS: Limits = Limits {
        entries: 100,
        assets: 0,
        storage_bytes: 5_000,
    };

    #[test]
    fn a_member_sees_each_limit_and_zero_means_unbounded() {
        let usage = usage_of(&held(false, LIMITS), true, 3);
        assert_eq!(
            usage,
            Usage {
                projects: Meter {
                    used: 1,
                    limit: Some(3)
                },
                entries: Meter {
                    used: 7,
                    limit: Some(100)
                },
                assets: Meter {
                    used: 2,
                    limit: None
                },
                storage_bytes: Meter {
                    used: 1_024,
                    limit: Some(5_000)
                },
            }
        );
    }

    #[test]
    fn an_administrator_is_unbounded_everywhere() {
        let usage = usage_of(&held(true, LIMITS), false, 3);
        for meter in [
            &usage.projects,
            &usage.entries,
            &usage.assets,
            &usage.storage_bytes,
        ] {
            assert_eq!(meter.limit, None, "{usage:?}");
        }
    }

    #[test]
    fn members_create_no_projects_where_registration_is_not_open() {
        assert_eq!(
            usage_of(&held(false, LIMITS), false, 3).projects.limit,
            Some(0)
        );
    }
}
