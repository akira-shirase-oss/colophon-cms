//! Administrators and the one-time bootstrap (docs/modules/03-auth.md, "Owner bootstrap"; D-043).
//! Administrators may create projects. The application role only reads these tables;
//! [`bootstrap`] runs as the migrator, from `colophon admin bootstrap`.

use sqlx::PgPool;
use uuid::Uuid;

use crate::DbError;

/// Where an identity stands with this installation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Standing {
    /// An administrator: may create projects.
    Administrator,
    /// Signed in, but not an administrator.
    Member,
    /// Nobody is an administrator yet: the installation hasn't been bootstrapped.
    NotBootstrapped,
}

/// Where `identity` stands.
pub async fn standing(pool: &PgPool, identity: Uuid) -> Result<Standing, DbError> {
    let row = sqlx::query!(
        r#"SELECT EXISTS (SELECT 1 FROM administrators WHERE identity_id = $1) AS "administrator!",
                  EXISTS (SELECT 1 FROM installation) AS "bootstrapped!""#,
        identity
    )
    .fetch_one(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(if row.administrator {
        Standing::Administrator
    } else if row.bootstrapped {
        Standing::Member
    } else {
        Standing::NotBootstrapped
    })
}

/// What a bootstrap did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Bootstrapped {
    /// The identity is now the installation's first administrator.
    Done {
        /// The identity, created if it had never signed in.
        identity: Uuid,
    },
    /// The installation was already bootstrapped for this same identity: nothing changed.
    AlreadyDone {
        /// The identity.
        identity: Uuid,
    },
    /// The installation was bootstrapped for someone else; bootstrap never runs twice.
    Refused,
}

/// Makes the identity `(issuer, subject)` the first administrator, once per installation. Runs
/// as the migrator (the application role can't write these tables); the identity is created if
/// it hasn't signed in yet, and its name filled in when it does.
pub async fn bootstrap(
    migrator: &PgPool,
    issuer: &str,
    subject: &str,
) -> Result<Bootstrapped, DbError> {
    let mut transaction = migrator.begin().await.map_err(DbError::from_query)?;
    let identity = sqlx::query_scalar!(
        "INSERT INTO identities (id, issuer, subject) VALUES ($1, $2, $3)
         ON CONFLICT (issuer, subject) DO UPDATE SET issuer = EXCLUDED.issuer
         RETURNING id",
        Uuid::now_v7(),
        issuer,
        subject
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    // A concurrent bootstrap waits here for the first to commit, then finds the row taken.
    let claimed = sqlx::query!(
        "INSERT INTO installation (bootstrapped_by) VALUES ($1) ON CONFLICT (singleton) DO NOTHING",
        identity
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    if claimed.rows_affected() == 0 {
        let by = sqlx::query_scalar!("SELECT bootstrapped_by FROM installation")
            .fetch_one(&mut *transaction)
            .await
            .map_err(DbError::from_query)?;
        // Nothing to keep: the identity upsert above is undone with the transaction.
        return Ok(if by == identity {
            Bootstrapped::AlreadyDone { identity }
        } else {
            Bootstrapped::Refused
        });
    }
    sqlx::query!(
        "INSERT INTO administrators (identity_id) VALUES ($1)",
        identity
    )
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Bootstrapped::Done { identity })
}
