//! OAuth 2.1 for the MCP endpoint (D-246): registered public clients, authorizations waiting for a
//! person's answer, grants, single-use codes and hashed tokens. None of these tables is
//! project-scoped for row security; the application reaches them only through these functions,
//! each by an id or a SHA-256, and every check that decides who may act is made here in one
//! statement or one transaction (a code's single use, a refresh token's rotation and its reuse).
use sqlx::PgPool;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::DbError;

/// The most clients the table holds; a registration past it is refused until old, never-used
/// clients are pruned (each registration prunes a few first).
pub const MAX_CLIENTS: i64 = 10_000;

/// A registered client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Client {
    /// Its id (the `client_id`).
    pub id: Uuid,
    /// What it called itself: a label nobody verified.
    pub name: String,
    /// Its exact redirect URIs.
    pub redirect_uris: Vec<String>,
}

/// An authorization waiting for its person's answer, as the consent page shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    /// The client asking.
    pub client: Client,
    /// Where the answer goes.
    pub redirect_uri: String,
    /// The client's `state`, echoed back.
    pub state: Option<String>,
    /// The PKCE challenge (S256) the code will be bound to.
    pub code_challenge: String,
}

/// A grant an access token acts under.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Grant {
    /// The grant's id.
    pub id: Uuid,
    /// The person it acts as.
    pub identity: Uuid,
    /// The one project it acts in.
    pub project: Uuid,
    /// The client it was given to.
    pub client: Uuid,
}

/// A code just redeemed: what it was bound to, for the caller to check against the request.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Redeemed {
    /// The grant it issues tokens for.
    pub grant: Grant,
    /// The redirect URI the authorization used.
    pub redirect_uri: String,
    /// The PKCE challenge.
    pub code_challenge: String,
}

/// What presenting a code or a refresh token found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Presented<T> {
    /// Valid and now used up.
    Taken(T),
    /// Unknown, expired, or its grant was revoked.
    Invalid,
    /// Used before: a replay. Its grant has just been revoked, with every token it issued.
    Replayed,
}

/// One of a person's grants, for "Connected apps".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Connected {
    /// The grant's id.
    pub id: Uuid,
    /// The client's own (unverified) name.
    pub client_name: String,
    /// The project it acts in, and its name.
    pub project: Uuid,
    /// The project's name.
    pub project_name: String,
    /// When it was approved.
    pub created_at: OffsetDateTime,
    /// When a token of it was last used.
    pub last_used_at: Option<OffsetDateTime>,
}

/// Registers a client, after pruning up to 50 clients that never got a grant and are over a day
/// old. `Ok(false)`: the table is full.
pub async fn register_client(
    pool: &PgPool,
    id: Uuid,
    name: &str,
    redirect_uris: &[String],
) -> Result<bool, DbError> {
    sqlx::query(
        "DELETE FROM oauth_clients WHERE id IN (
             SELECT c.id FROM oauth_clients c
             WHERE c.created_at < now() - interval '1 day' AND c.last_used_at IS NULL
               AND NOT EXISTS (SELECT FROM oauth_grants g WHERE g.client_id = c.id)
             ORDER BY c.created_at LIMIT 50)",
    )
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    let inserted = sqlx::query(
        "INSERT INTO oauth_clients (id, name, redirect_uris)
         SELECT $1, $2, $3 WHERE (SELECT count(*) FROM oauth_clients) < $4",
    )
    .bind(id)
    .bind(name)
    .bind(redirect_uris)
    .bind(MAX_CLIENTS)
    .execute(pool)
    .await
    .map_err(DbError::from_write)?;
    Ok(inserted.rows_affected() == 1)
}

/// The client with this id.
pub async fn client(pool: &PgPool, id: Uuid) -> Result<Option<Client>, DbError> {
    let row: Option<(Uuid, String, Vec<String>)> =
        sqlx::query_as("SELECT id, name, redirect_uris FROM oauth_clients WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(DbError::from_query)?;
    Ok(row.map(|(id, name, redirect_uris)| Client {
        id,
        name,
        redirect_uris,
    }))
}

/// Records an authorization for `identity` to answer, valid ten minutes; its id.
pub async fn start_request(
    pool: &PgPool,
    client: Uuid,
    redirect_uri: &str,
    state: Option<&str>,
    code_challenge: &str,
    identity: Uuid,
) -> Result<Uuid, DbError> {
    let id = Uuid::now_v7();
    sqlx::query("DELETE FROM oauth_requests WHERE expires_at < now() - interval '1 hour'")
        .execute(pool)
        .await
        .map_err(DbError::from_query)?;
    sqlx::query(
        "INSERT INTO oauth_requests (id, client_id, redirect_uri, state, code_challenge, identity_id, expires_at)
         VALUES ($1, $2, $3, $4, $5, $6, now() + interval '10 minutes')",
    )
    .bind(id)
    .bind(client)
    .bind(redirect_uri)
    .bind(state)
    .bind(code_challenge)
    .bind(identity)
    .execute(pool)
    .await
    .map_err(DbError::from_write)?;
    Ok(id)
}

/// The authorization `id`, if it is `identity`'s and hasn't expired.
pub async fn pending(pool: &PgPool, id: Uuid, identity: Uuid) -> Result<Option<Pending>, DbError> {
    let row: Option<(Uuid, String, Vec<String>, String, Option<String>, String)> = sqlx::query_as(
        "SELECT c.id, c.name, c.redirect_uris, r.redirect_uri, r.state, r.code_challenge
         FROM oauth_requests r JOIN oauth_clients c ON c.id = r.client_id
         WHERE r.id = $1 AND r.identity_id = $2 AND r.expires_at > now()",
    )
    .bind(id)
    .bind(identity)
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.map(|(client, name, redirect_uris, redirect_uri, state, code_challenge)| Pending {
        client: Client {
            id: client,
            name,
            redirect_uris,
        },
        redirect_uri,
        state,
        code_challenge,
    }))
}

/// Ends the authorization `id` (`identity`'s, unexpired), and with `project` approves it: one
/// transaction makes the grant and a code (its hash), valid ten minutes. The request is consumed
/// either way, so an answer counts once. `None` when there is no such request; the caller has
/// checked that `identity` may act in `project`.
pub async fn answer(
    pool: &PgPool,
    id: Uuid,
    identity: Uuid,
    approve: Option<(Uuid, &[u8; 32])>,
) -> Result<Option<Pending>, DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    let row: Option<(Uuid, String, Option<String>, String)> = sqlx::query_as(
        "DELETE FROM oauth_requests WHERE id = $1 AND identity_id = $2 AND expires_at > now()
         RETURNING client_id, redirect_uri, state, code_challenge",
    )
    .bind(id)
    .bind(identity)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some((client, redirect_uri, state, code_challenge)) = row else {
        return Ok(None);
    };
    if let Some((project, code_hash)) = approve {
        let grant = Uuid::now_v7();
        sqlx::query("INSERT INTO oauth_grants (id, client_id, identity_id, project_id) VALUES ($1, $2, $3, $4)")
            .bind(grant)
            .bind(client)
            .bind(identity)
            .bind(project)
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_write)?;
        sqlx::query(
            "INSERT INTO oauth_codes (code_hash, grant_id, redirect_uri, code_challenge, expires_at)
             VALUES ($1, $2, $3, $4, now() + interval '10 minutes')",
        )
        .bind(&code_hash[..])
        .bind(grant)
        .bind(&redirect_uri)
        .bind(&code_challenge)
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_write)?;
    }
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(Some(Pending {
        client: Client {
            id: client,
            name: String::new(),
            redirect_uris: Vec::new(),
        },
        redirect_uri,
        state,
        code_challenge,
    }))
}

/// Revokes `grant` (and so every token it issued). Idempotent.
async fn revoke(executor: impl sqlx::PgExecutor<'_>, grant: Uuid) -> Result<(), DbError> {
    sqlx::query("UPDATE oauth_grants SET revoked_at = now() WHERE id = $1 AND revoked_at IS NULL")
        .bind(grant)
        .execute(executor)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

/// Uses up the code with this hash. A code presented a second time revokes its grant: the first
/// use may have been an attacker's (RFC 6749 §4.1.2).
pub async fn redeem_code(pool: &PgPool, code_hash: &[u8; 32]) -> Result<Presented<Redeemed>, DbError> {
    let taken: Option<(Uuid, Uuid, Uuid, Uuid, String, String)> = sqlx::query_as(
        "UPDATE oauth_codes c SET used_at = now()
         FROM oauth_grants g
         WHERE c.code_hash = $1 AND c.used_at IS NULL AND c.expires_at > now()
           AND g.id = c.grant_id AND g.revoked_at IS NULL
         RETURNING g.id, g.identity_id, g.project_id, g.client_id, c.redirect_uri, c.code_challenge",
    )
    .bind(&code_hash[..])
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    if let Some((id, identity, project, client, redirect_uri, code_challenge)) = taken {
        return Ok(Presented::Taken(Redeemed {
            grant: Grant {
                id,
                identity,
                project,
                client,
            },
            redirect_uri,
            code_challenge,
        }));
    }
    let used: Option<(Uuid,)> =
        sqlx::query_as("SELECT grant_id FROM oauth_codes WHERE code_hash = $1 AND used_at IS NOT NULL")
            .bind(&code_hash[..])
            .fetch_optional(pool)
            .await
            .map_err(DbError::from_query)?;
    if let Some((grant,)) = used {
        revoke(pool, grant).await?;
        return Ok(Presented::Replayed);
    }
    Ok(Presented::Invalid)
}

/// Stores a fresh access token (an hour) and refresh token (thirty days) for `grant`, and drops
/// the grant's expired ones.
pub async fn issue(
    pool: &PgPool,
    grant: Uuid,
    access_hash: &[u8; 32],
    refresh_hash: &[u8; 32],
) -> Result<(), DbError> {
    let mut transaction = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    sqlx::query("DELETE FROM oauth_tokens WHERE grant_id = $1 AND expires_at < now()")
        .bind(grant)
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    sqlx::query(
        "INSERT INTO oauth_tokens (token_hash, grant_id, kind, expires_at) VALUES
         ($1, $3, 'access', now() + interval '1 hour'),
         ($2, $3, 'refresh', now() + interval '30 days')",
    )
    .bind(&access_hash[..])
    .bind(&refresh_hash[..])
    .bind(grant)
    .execute(&mut *transaction)
    .await
    .map_err(DbError::from_write)?;
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(())
}

/// Uses up the refresh token with this hash, if it is `client`'s. One presented again after its use
/// revokes its grant (a stolen copy and the real one can't both go on).
pub async fn redeem_refresh(
    pool: &PgPool,
    refresh_hash: &[u8; 32],
    client: Uuid,
) -> Result<Presented<Grant>, DbError> {
    let taken: Option<(Uuid, Uuid, Uuid, Uuid)> = sqlx::query_as(
        "UPDATE oauth_tokens t SET used_at = now()
         FROM oauth_grants g
         WHERE t.token_hash = $1 AND t.kind = 'refresh' AND t.used_at IS NULL AND t.expires_at > now()
           AND g.id = t.grant_id AND g.revoked_at IS NULL AND g.client_id = $2
         RETURNING g.id, g.identity_id, g.project_id, g.client_id",
    )
    .bind(&refresh_hash[..])
    .bind(client)
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    if let Some((id, identity, project, client)) = taken {
        return Ok(Presented::Taken(Grant {
            id,
            identity,
            project,
            client,
        }));
    }
    let used: Option<(Uuid,)> = sqlx::query_as(
        "SELECT grant_id FROM oauth_tokens WHERE token_hash = $1 AND kind = 'refresh' AND used_at IS NOT NULL",
    )
    .bind(&refresh_hash[..])
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    if let Some((grant,)) = used {
        revoke(pool, grant).await?;
        return Ok(Presented::Replayed);
    }
    Ok(Presented::Invalid)
}

/// The grant the access token with this hash acts under: unexpired, its grant unrevoked and its
/// person's account enabled. Records the grant's use at most once a minute.
pub async fn access(pool: &PgPool, access_hash: &[u8; 32]) -> Result<Option<Grant>, DbError> {
    let row: Option<(Uuid, Uuid, Uuid, Uuid, Option<OffsetDateTime>)> = sqlx::query_as(
        "SELECT g.id, g.identity_id, g.project_id, g.client_id, g.last_used_at
         FROM oauth_tokens t
         JOIN oauth_grants g ON g.id = t.grant_id AND g.revoked_at IS NULL
         JOIN identities i ON i.id = g.identity_id AND i.disabled_at IS NULL
         WHERE t.token_hash = $1 AND t.kind = 'access' AND t.expires_at > now()",
    )
    .bind(&access_hash[..])
    .fetch_optional(pool)
    .await
    .map_err(DbError::from_query)?;
    let Some((id, identity, project, client, last_used)) = row else {
        return Ok(None);
    };
    if last_used.is_none_or(|at| OffsetDateTime::now_utc() - at >= time::Duration::MINUTE) {
        let touched = sqlx::query("UPDATE oauth_grants SET last_used_at = now() WHERE id = $1")
            .bind(id)
            .execute(pool)
            .await;
        if let Err(error) = touched {
            tracing::warn!(%error, "could not record an OAuth grant's use");
        }
    }
    Ok(Some(Grant {
        id,
        identity,
        project,
        client,
    }))
}

/// Revokes the grant of the token with this hash, access or refresh (RFC 7009): the client's
/// whole connection ends. Unknown tokens are not an error.
pub async fn revoke_token(pool: &PgPool, token_hash: &[u8; 32]) -> Result<(), DbError> {
    let grant: Option<(Uuid,)> = sqlx::query_as("SELECT grant_id FROM oauth_tokens WHERE token_hash = $1")
        .bind(&token_hash[..])
        .fetch_optional(pool)
        .await
        .map_err(DbError::from_query)?;
    if let Some((grant,)) = grant {
        revoke(pool, grant).await?;
    }
    Ok(())
}

/// `identity`'s grants that still stand, newest first.
pub async fn connected(pool: &PgPool, identity: Uuid) -> Result<Vec<Connected>, DbError> {
    let rows: Vec<(Uuid, String, Uuid, String, OffsetDateTime, Option<OffsetDateTime>)> = sqlx::query_as(
        "SELECT g.id, c.name, g.project_id, p.name, g.created_at, g.last_used_at
         FROM oauth_grants g
         JOIN oauth_clients c ON c.id = g.client_id
         JOIN projects p ON p.id = g.project_id
         WHERE g.identity_id = $1 AND g.revoked_at IS NULL
         ORDER BY g.created_at DESC",
    )
    .bind(identity)
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(rows
        .into_iter()
        .map(|(id, client_name, project, project_name, created_at, last_used_at)| Connected {
            id,
            client_name,
            project,
            project_name,
            created_at,
            last_used_at,
        })
        .collect())
}

/// Revokes `identity`'s grant `grant`; `false` when it has no such grant standing.
pub async fn revoke_grant(pool: &PgPool, identity: Uuid, grant: Uuid) -> Result<bool, DbError> {
    let done = sqlx::query(
        "UPDATE oauth_grants SET revoked_at = now() WHERE id = $1 AND identity_id = $2 AND revoked_at IS NULL",
    )
    .bind(grant)
    .bind(identity)
    .execute(pool)
    .await
    .map_err(DbError::from_query)?;
    Ok(done.rows_affected() == 1)
}

/// Revokes every grant of `identity` (a changed password: whoever held a connection loses it).
pub async fn revoke_all(executor: impl sqlx::PgExecutor<'_>, identity: Uuid) -> Result<u64, DbError> {
    let done = sqlx::query("UPDATE oauth_grants SET revoked_at = now() WHERE identity_id = $1 AND revoked_at IS NULL")
        .bind(identity)
        .execute(executor)
        .await
        .map_err(DbError::from_query)?;
    Ok(done.rows_affected())
}
