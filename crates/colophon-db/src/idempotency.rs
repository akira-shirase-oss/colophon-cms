//! Idempotency records (docs/modules/02-db.md, "Constraints that carry invariants"): the same
//! request with the same key gets the recorded answer; a different request under that key is
//! refused. Scoped to the identity, the project, the environment and the operation.

use crate::{DbError, scope::ProjectTransaction};

/// What a claim found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Claim {
    /// The key is new: do the work, then [`complete`] in the same transaction.
    Fresh,
    /// The same request already succeeded: answer with this.
    Replay {
        /// The HTTP status it got.
        status: u16,
        /// The response body it got.
        response: String,
    },
    /// The key was used for a different request.
    Mismatch,
}

/// Claims `key` for this request. If another transaction holds the same key, this waits for it
/// to finish, then replays its answer or, if it rolled back, claims the key itself.
pub async fn claim(
    tx: &mut ProjectTransaction,
    operation: &str,
    key: &str,
    request_hash: &[u8; 32],
) -> Result<Claim, DbError> {
    let claimed = sqlx::query_scalar!(
        "INSERT INTO idempotency_records (project_id, environment_id, identity_id, operation, key, request_hash)
         VALUES ($1, $2, $3, $4, $5, $6)
         ON CONFLICT DO NOTHING
         RETURNING true AS \"claimed!\"",
        tx.project,
        tx.environment,
        tx.identity,
        operation,
        key,
        &request_hash[..]
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    if claimed.is_some() {
        return Ok(Claim::Fresh);
    }
    let record = sqlx::query!(
        "SELECT request_hash, status, response FROM idempotency_records
         WHERE project_id = $1 AND environment_id = $2 AND identity_id = $3 AND operation = $4 AND key = $5",
        tx.project,
        tx.environment,
        tx.identity,
        operation,
        key
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    if record.request_hash != request_hash {
        return Ok(Claim::Mismatch);
    }
    match (
        record.status.and_then(|status| u16::try_from(status).ok()),
        record.response,
    ) {
        (Some(status), Some(response)) => Ok(Claim::Replay { status, response }),
        // A committed claim always carries its answer; the schema's CHECK keeps them paired.
        _ => Err(DbError::Corrupt(
            "an idempotency record committed without its response",
        )),
    }
}

/// Records the answer for a key claimed in this transaction.
pub async fn complete(
    tx: &mut ProjectTransaction,
    operation: &str,
    key: &str,
    status: u16,
    response: &str,
) -> Result<(), DbError> {
    let status = i16::try_from(status).map_err(|_| DbError::Corrupt("status out of range"))?;
    sqlx::query!(
        "UPDATE idempotency_records SET status = $6, response = $7
         WHERE project_id = $1 AND environment_id = $2 AND identity_id = $3 AND operation = $4 AND key = $5",
        tx.project,
        tx.environment,
        tx.identity,
        operation,
        key,
        status,
        response
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}
