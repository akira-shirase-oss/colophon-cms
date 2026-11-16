//! The deletion ledger (docs/modules/02-db.md, "The deletion ledger"; migration 0048, D-124).
//!
//! Every path that permanently removes a governed object appends a row in the same transaction,
//! so a deletion and its record commit together or not at all. The ledger is the off-host record
//! a restore replays (spec §33, restore step 4; invariant O2): a backup restored from before a
//! deletion resurrects the object as it was, and the replayed interval removes it again — F14's
//! "deleted project appears in a restored backup".
//!
//! Writes ride inside the delete paths: `colophon_delete_entry` and `colophon_delete_project`
//! insert their rows in SQL; the Rust delete paths ([`crate::content_types::remove`],
//! [`crate::members::remove`], [`crate::roles::delete`], the worker's asset-`deleted` outcome)
//! call [`record_in`]. Nothing reads the ledger through the application roles; [`high_water`]
//! and [`page`] run as the migrator, which row-level security does not force, for the off-host
//! export.
//!
//! [`replay`] applies an exported interval onto a freshly restored database, before read APIs
//! and caches are enabled (O2): it runs as `colophon_deleter`, which is why the migrator holds a
//! `SET TRUE` membership in that role. It reinserts each ledger row verbatim (its own id, so the
//! restored ledger converges on the export) and reapplies the deletion, idempotently: a row that
//! names something already gone is skipped, and the whole call can run twice. A row it cannot
//! apply is reported, never silently absorbed — restore step 4's "the required ledger interval
//! can't be reconstructed" surfaces as a [`Conflict`].

use std::fmt::Write as _;
use std::io::Write;

use sqlx::{Postgres, Transaction};
use time::{OffsetDateTime, format_description::well_known::Rfc3339};
use uuid::Uuid;

use crate::{DbError, PgPool};

/// The identity replay attributes its writes to (migration 0048; like the worker's, 0045).
pub const RESTORER: Uuid = Uuid::from_u128(0xa1b2c3d4_0000_4000_8000_000000000007);

/// The most rows one export page or replay batch holds.
pub const PAGE: i64 = 1000;

/// What kind of governed object a ledger row deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A whole project (`colophon_delete_project`).
    Project,
    /// A collection and every version of its definition (`content_types::remove`, D-083).
    Collection,
    /// An entry's tombstone (`colophon_delete_entry`, D-080).
    Entry,
    /// A membership (`members::remove`): a removal and a member leaving record the same row.
    Member,
    /// A custom role (`roles::delete`); the fixed owner role can never reach the ledger.
    Role,
    /// An asset's terminal `deleted` state, landed by the worker after its objects went.
    Asset,
}

impl Kind {
    /// The `kind` column's spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Collection => "collection",
            Self::Entry => "entry",
            Self::Member => "member",
            Self::Role => "role",
            Self::Asset => "asset",
        }
    }

    /// What the column stores, or `None` for anything else — corrupt data, not a kind to skip.
    pub fn parse(kind: &str) -> Option<Self> {
        match kind {
            "project" => Some(Self::Project),
            "collection" => Some(Self::Collection),
            "entry" => Some(Self::Entry),
            "member" => Some(Self::Member),
            "role" => Some(Self::Role),
            "asset" => Some(Self::Asset),
            _ => None,
        }
    }
}

/// One ledger row, as `deletion_ledger` stores it and the export carries it. `detail` is a JSON
/// object as text, as with the audit log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Row {
    /// Its place: monotone, so a high-water mark bounds an export.
    pub id: i64,
    /// When the deletion committed.
    pub deleted_at: OffsetDateTime,
    /// What was deleted.
    pub kind: Kind,
    /// The project it belonged to — not a foreign key: the row outlives its project.
    pub project_id: Uuid,
    /// For humans reading the file off-host.
    pub project_slug: String,
    /// The environment for environment-scoped kinds.
    pub environment_id: Option<Uuid>,
    /// The deleted object, enough to reapply it: `{collection, entry}`, `{identity}`, `{role}`…
    pub detail: String,
}

/// Records a deletion of `kind` in the caller's transaction: [`crate::content_types::remove`],
/// [`crate::members::remove`], [`crate::roles::delete`] and the worker's asset outcome call it.
/// `detail` is a JSON object as text, like the audit log's. The project's slug is read with the
/// write, so the row still names its project after the project is gone. There is no `RETURNING`:
/// the writers hold INSERT only — a RETURNING would ask for SELECT, and nothing may read this
/// table through the application roles.
pub async fn record_in(
    transaction: &mut Transaction<'static, Postgres>,
    kind: Kind,
    project: Uuid,
    environment: Option<Uuid>,
    detail: &str,
) -> Result<(), DbError> {
    sqlx::query!(
        r#"INSERT INTO deletion_ledger (kind, project_id, project_slug, environment_id, detail)
           VALUES ($1, $2, (SELECT slug FROM projects WHERE id = $2), $3, $4::text::jsonb)"#,
        kind.as_str(),
        project,
        environment,
        detail
    )
    .execute(&mut **transaction)
    .await
    .map_err(DbError::from_write)?;
    Ok(())
}

/// The highest id the ledger holds, `0` when empty: an export's watermark. Rows committed later
/// can only land above it, so `id <= high_water` at export time is a complete interval — with
/// one caveat D-124 records: a row that drew a low id but commits after the watermark waits for
/// the next export.
pub async fn high_water(pool: &PgPool) -> Result<i64, DbError> {
    sqlx::query_scalar!("SELECT coalesce(max(id), 0) AS \"mark!\" FROM deletion_ledger")
        .fetch_one(pool)
        .await
        .map_err(DbError::from_query)
}

/// Rows with `after < id <= up_to`, id-ordered, at most `limit` (capped at [`PAGE`]): one export
/// page. Callers page until a page comes back short. Runs as the migrator, so it sees every
/// project's rows.
pub async fn page(pool: &PgPool, after: i64, up_to: i64, limit: i64) -> Result<Vec<Row>, DbError> {
    let rows = sqlx::query!(
        r#"SELECT id, deleted_at, kind, project_id, project_slug, environment_id, detail::text AS "detail!"
           FROM deletion_ledger
           WHERE id > $1 AND id <= $2
           ORDER BY id
           LIMIT $3"#,
        after,
        up_to,
        limit.clamp(1, PAGE)
    )
    .fetch_all(pool)
    .await
    .map_err(DbError::from_query)?;
    rows.into_iter()
        .map(|row| {
            Ok(Row {
                id: row.id,
                deleted_at: row.deleted_at,
                kind: Kind::parse(&row.kind)
                    .ok_or(DbError::Corrupt("a ledger row holds an unknown kind"))?,
                project_id: row.project_id,
                project_slug: row.project_slug,
                environment_id: row.environment_id,
                detail: row.detail,
            })
        })
        .collect()
}

/// The export's first line, then one [`Row`] a line: a versioned JSONL snapshot. Writing is the
/// caller's `sink`, one bounded line at a time, so a ledger of any size exports with the memory
/// of one row. `after` picks up where the last export's watermark ended.
pub async fn export(
    pool: &PgPool,
    after: i64,
    sink: &mut impl Write,
) -> Result<Exported, LedgerError> {
    let mark = high_water(pool).await?;
    let header = serde_json::json!({
        "format": "colophon/deletion-ledger",
        "version": 1,
        "high_water": mark,
        "after": after,
    });
    write_line(sink, &header.to_string())?;
    let mut rows = 0u64;
    let mut cursor = after;
    loop {
        let page = page(pool, cursor, mark, PAGE).await?;
        // PAGE is a positive constant; on failure the page reads as full so export keeps going.
        let short = page.len() < usize::try_from(PAGE).unwrap_or(usize::MAX);
        for row in page {
            cursor = row.id;
            write_line(sink, &line(&row)?)?;
            rows += 1;
        }
        if short {
            break;
        }
    }
    Ok(Exported {
        high_water: mark,
        rows,
    })
}

/// One row's export line.
fn line(row: &Row) -> Result<String, LedgerError> {
    let detail: serde_json::Value =
        serde_json::from_str(&row.detail).map_err(|_| LedgerError::CorruptDetail(row.id))?;
    let deleted_at = row
        .deleted_at
        .format(&Rfc3339)
        .map_err(|_| LedgerError::CorruptDetail(row.id))?;
    Ok(serde_json::json!({
        "id": row.id,
        "deleted_at": deleted_at,
        "kind": row.kind.as_str(),
        "project_id": row.project_id,
        "project_slug": row.project_slug,
        "environment_id": row.environment_id,
        "detail": detail,
    })
    .to_string())
}

fn write_line(sink: &mut impl Write, line: &str) -> Result<(), LedgerError> {
    sink.write_all(line.as_bytes())
        .and_then(|()| sink.write_all(b"\n"))
        .map_err(LedgerError::Sink)
}

/// Parses an export's lines into the header's watermark and the rows [`replay`] takes. Every
/// line must be one written by [`export`] — a short file, a reordered id or an unknown kind is
/// a corrupt interval, which O2 says blocks the restore rather than replaying partially.
pub fn unmarshal(bytes: &[u8]) -> Result<Unmarshalled, UnmarshalError> {
    let text = std::str::from_utf8(bytes).map_err(|_| UnmarshalError::NotUtf8)?;
    let mut lines = text.lines().enumerate();
    let (number, header) = lines.next().ok_or(UnmarshalError::MissingHeader)?;
    let header: serde_json::Value =
        serde_json::from_str(header).map_err(|_| UnmarshalError::BadLine(number + 1))?;
    if header.get("format").and_then(serde_json::Value::as_str) != Some("colophon/deletion-ledger")
        || header.get("version").and_then(serde_json::Value::as_i64) != Some(1)
    {
        return Err(UnmarshalError::BadHeader);
    }
    let high_water = header
        .get("high_water")
        .and_then(serde_json::Value::as_i64)
        .ok_or(UnmarshalError::BadHeader)?;
    let mut rows = Vec::new();
    let mut last = 0i64;
    for (number, line) in lines {
        let parsed: serde_json::Value =
            serde_json::from_str(line).map_err(|_| UnmarshalError::BadLine(number + 1))?;
        let id = parsed
            .get("id")
            .and_then(serde_json::Value::as_i64)
            .ok_or(UnmarshalError::BadLine(number + 1))?;
        if id <= last {
            return Err(UnmarshalError::OutOfOrder(number + 1));
        }
        last = id;
        let kind = Kind::parse(
            parsed
                .get("kind")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
        )
        .ok_or(UnmarshalError::BadLine(number + 1))?;
        let deleted_at = OffsetDateTime::parse(
            parsed
                .get("deleted_at")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default(),
            &Rfc3339,
        )
        .map_err(|_| UnmarshalError::BadLine(number + 1))?;
        let project_id = uuid_at(&parsed, "project_id", number + 1)?;
        let project_slug = parsed
            .get("project_slug")
            .and_then(serde_json::Value::as_str)
            .ok_or(UnmarshalError::BadLine(number + 1))?
            .to_owned();
        let environment_id = match parsed.get("environment_id") {
            None | Some(serde_json::Value::Null) => None,
            Some(_) => Some(uuid_at(&parsed, "environment_id", number + 1)?),
        };
        let detail = parsed
            .get("detail")
            .filter(|value| value.is_object())
            .ok_or(UnmarshalError::BadLine(number + 1))?
            .to_string();
        rows.push(Row {
            id,
            deleted_at,
            kind,
            project_id,
            project_slug,
            environment_id,
            detail,
        });
    }
    Ok(Unmarshalled { high_water, rows })
}

fn uuid_at(value: &serde_json::Value, key: &str, line: usize) -> Result<Uuid, UnmarshalError> {
    Uuid::parse_str(
        value
            .get(key)
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default(),
    )
    .map_err(|_| UnmarshalError::BadLine(line))
}

/// An export's lines, ready for [`replay`]: `high_water` is the completeness check the restore
/// procedure compares against its backup's cut point (O2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unmarshalled {
    /// The watermark the export covered.
    pub high_water: i64,
    /// The interval's rows, id-ordered as the file held them.
    pub rows: Vec<Row>,
}

/// Why an export refused to parse.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum UnmarshalError {
    /// The file isn't UTF-8 text.
    #[error("the ledger export is not UTF-8")]
    NotUtf8,
    /// There is no header line.
    #[error("the ledger export has no header line")]
    MissingHeader,
    /// The header names another format or version.
    #[error("the ledger export's header names another format or version")]
    BadHeader,
    /// A line isn't the JSON [`export`] writes (1-based line number).
    #[error("line {0} of the ledger export is not a written row")]
    BadLine(usize),
    /// Ids must ascend; the file was truncated or reordered (1-based line number).
    #[error("line {0} of the ledger export is out of id order")]
    OutOfOrder(usize),
}

/// What an [`export`] wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Exported {
    /// The watermark the file covers: every row with `id <= high_water` is in it.
    pub high_water: i64,
    /// How many rows.
    pub rows: u64,
}

/// An [`export`] failure: the database, the sink, or a stored detail that isn't JSON.
#[derive(Debug, thiserror::Error)]
pub enum LedgerError {
    /// The database read failed.
    #[error("{0}")]
    Db(#[from] DbError),
    /// The sink refused a line.
    #[error("the export sink failed")]
    Sink(#[source] std::io::Error),
    /// A stored detail isn't the JSON object the CHECK keeps.
    #[error("ledger row {0} holds a detail that is not JSON")]
    CorruptDetail(i64),
}

/// What a [`replay`] did with the rows it was given.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Replayed {
    /// Rows whose deletion was reapplied.
    pub applied: u64,
    /// Rows naming something already gone — replay is idempotent, so this is normal.
    pub skipped: u64,
    /// Rows the database refused to replay, each with why. Restore step 4 blocks on any.
    pub conflicts: Vec<Conflict>,
}

/// A row the replay could not apply.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Conflict {
    /// The ledger row's id.
    pub id: i64,
    /// What went wrong, without content (the database's message can carry constraint names).
    pub reason: String,
}

/// Reapplies ledger `rows` — in id order, so cause deletions land before the deletions they
/// enabled (a member removal before the role delete it unblocked) — onto a freshly restored
/// database. Runs as `colophon_deleter` inside one transaction, so it must be called as the
/// migrator, and before anything serves (O2): the replay is the deletion history the backup
/// predates, and serving first would answer with resurrected objects.
///
/// Each row is copied into the restored ledger with its own id first (`OVERRIDING SYSTEM
/// VALUE`, `ON CONFLICT DO NOTHING` — a restored database may already hold some of the
/// interval), then reapplied per kind under a savepoint: an entry is force-tombstoned and its
/// unique claims freed exactly as `colophon_delete_entry` leaves them, a collection's versions
/// and row go, a membership goes, a custom role goes, an asset is marked `deleted`, a project
/// runs the same `colophon_delete_project` a live deletion does. A row naming something absent
/// or already in its end state is skipped. A row that errors reports a [`Conflict`] and the
/// replay continues; the caller decides whether the restore may proceed.
pub async fn replay(pool: &PgPool, rows: &[Row]) -> Result<Replayed, DbError> {
    if rows.is_empty() {
        return Ok(Replayed::default());
    }
    let mut transaction = pool.begin().await.map_err(DbError::from_query)?;
    // Rows come in with their own ids, which don't advance the sequence — and a replayed
    // project deletion writes a fresh row of its own, which draws it. It must start above
    // both the interval and whatever an earlier replay left, before the role switch;
    // sequences ignore rollback, so a failed replay only ever leaves a harmless gap.
    sqlx::query_scalar!(
        r#"SELECT setval(pg_get_serial_sequence('deletion_ledger', 'id'),
                        (SELECT greatest($1, coalesce(max(id), 1)) FROM deletion_ledger)) AS "set!""#,
        rows.iter().map(|row| row.id).max().unwrap_or(1)
    )
    .fetch_one(&mut *transaction)
    .await
    .map_err(DbError::from_query)?;
    sqlx::query("SET LOCAL ROLE colophon_deleter")
        .execute(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    // Flags the last-owner check yields to (migration 0048).
    sqlx::query!("SELECT set_config('app.ledger_replay', 'on', true) AS \"replaying!\"")
        .fetch_one(&mut *transaction)
        .await
        .map_err(DbError::from_query)?;
    let mut replayed = Replayed::default();
    for row in rows {
        sqlx::query("SAVEPOINT replay_row")
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_query)?;
        match apply(&mut transaction, row).await {
            Ok(true) => replayed.applied += 1,
            Ok(false) => replayed.skipped += 1,
            Err(reason) => {
                sqlx::query("ROLLBACK TO SAVEPOINT replay_row")
                    .execute(&mut *transaction)
                    .await
                    .map_err(DbError::from_query)?;
                replayed.conflicts.push(Conflict { id: row.id, reason });
            }
        }
        sqlx::query("RELEASE SAVEPOINT replay_row")
            .execute(&mut *transaction)
            .await
            .map_err(DbError::from_query)?;
    }
    transaction.commit().await.map_err(DbError::from_query)?;
    Ok(replayed)
}

/// Reapplies one row under the caller's savepoint: `true` when it changed something, `false`
/// when the object was already gone, or why it couldn't (`Err` — the caller rolls back to the
/// savepoint). The ledger row itself is copied first, keeping the restored ledger's interval
/// true with the export's.
async fn apply(tx: &mut Transaction<'static, Postgres>, row: &Row) -> Result<bool, String> {
    sqlx::query!(
        r#"SELECT set_config('app.project_id', $1, true) AS "p!", set_config('app.identity_id', $2, true) AS "i!""#,
        row.project_id.to_string(),
        RESTORER.to_string()
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(short)?;
    // The row is copied with its own id through the migrator-owned definer (migration 0048):
    // ON CONFLICT reads the conflict column, a SELECT the replay role may never hold here.
    sqlx::query!(
        r#"SELECT colophon_ledger_row($1, $2, $3, $4, $5, $6, $7::text::jsonb) AS "copied!""#,
        row.id,
        row.deleted_at,
        row.kind.as_str(),
        row.project_id,
        row.project_slug,
        row.environment_id,
        row.detail
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(short)?;
    let detail: serde_json::Value =
        serde_json::from_str(&row.detail).map_err(|_| "detail is not a JSON object".to_owned())?;
    let applied = match row.kind {
        Kind::Entry => apply_entry(tx, row, &detail).await?,
        Kind::Collection => apply_collection(tx, row, &detail).await?,
        Kind::Member => apply_member(tx, row, &detail).await?,
        Kind::Role => apply_role(tx, row, &detail).await?,
        Kind::Asset => apply_asset(tx, row, &detail).await?,
        Kind::Project => apply_project(tx, row).await?,
    };
    Ok(applied)
}

/// The error text for a failed row, trimmed at the message — never the detail.
fn short(error: sqlx::Error) -> String {
    match error {
        sqlx::Error::Database(database) => {
            let mut reason = database.message().to_owned();
            if let Some(constraint) = database.constraint() {
                let _ = write!(reason, " ({constraint})");
            }
            reason
        }
        error => error.to_string(),
    }
}

/// Reads `key` of `detail` as a uuid or a string; a missing or wrong-shaped field is why the
/// row fails, not a panic.
fn field<'a>(detail: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    detail
        .get(key)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| format!("detail has no {key}"))
}

fn uuid_field(detail: &serde_json::Value, key: &str) -> Result<Uuid, String> {
    Uuid::parse_str(field(detail, key)?).map_err(|_| format!("detail's {key} is not a uuid"))
}

/// An entry is force-tombstoned exactly as `colophon_delete_entry` leaves it — the guards that
/// function checks (published, referenced, pinned) protect live traffic, and a restored backup
/// is stale state, not traffic. Its unique values come free the same way. `false` when it is
/// already a tombstone or absent.
async fn apply_entry(
    tx: &mut Transaction<'static, Postgres>,
    row: &Row,
    detail: &serde_json::Value,
) -> Result<bool, String> {
    let entry = uuid_field(detail, "entry")?;
    let environment = row
        .environment_id
        .ok_or_else(|| "an entry row has no environment".to_owned())?;
    let changed = sqlx::query!(
        "UPDATE entries SET deleted_at = $4, deleted_by = $5
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND deleted_at IS NULL",
        row.project_id,
        environment,
        entry,
        row.deleted_at,
        RESTORER
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    if changed.rows_affected() == 0 {
        return Ok(false);
    }
    sqlx::query!(
        "DELETE FROM unique_claims WHERE project_id = $1 AND environment_id = $2 AND entry_id = $3",
        row.project_id,
        environment,
        entry
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    replayed_audit(tx, row, &entry.to_string()).await?;
    Ok(true)
}

/// A collection's versions and row go, as `content_types::remove` leaves them. The ledger row's
/// commit proves its guards held then, and revisions are insert-only, so a restored backup can
/// hold nothing that would pin them.
async fn apply_collection(
    tx: &mut Transaction<'static, Postgres>,
    row: &Row,
    detail: &serde_json::Value,
) -> Result<bool, String> {
    let collection = field(detail, "collection")?;
    let environment = row
        .environment_id
        .ok_or_else(|| "a collection row has no environment".to_owned())?;
    sqlx::query!(
        "DELETE FROM content_type_versions WHERE project_id = $1 AND environment_id = $2 AND collection = $3",
        row.project_id,
        environment,
        collection
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    let changed = sqlx::query!(
        "DELETE FROM collections WHERE project_id = $1 AND environment_id = $2 AND slug = $3",
        row.project_id,
        environment,
        collection
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    if changed.rows_affected() > 0 {
        replayed_audit(tx, row, collection).await?;
    }
    Ok(changed.rows_affected() > 0)
}

/// The membership goes. The deferred last-owner check yields to the replay's flag (migration
/// 0048): a backup can hold a last owner whose removal the live database refused to keep
/// ownerless only because a later member — absent from the backup — had joined since.
async fn apply_member(
    tx: &mut Transaction<'static, Postgres>,
    row: &Row,
    detail: &serde_json::Value,
) -> Result<bool, String> {
    let identity = uuid_field(detail, "identity")?;
    let changed = sqlx::query!(
        "DELETE FROM memberships WHERE project_id = $1 AND identity_id = $2",
        row.project_id,
        identity
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    if changed.rows_affected() > 0 {
        replayed_audit(tx, row, &identity.to_string()).await?;
    }
    Ok(changed.rows_affected() > 0)
}

/// The custom role goes; its permission rows follow the cascade. The owner role can never
/// reach the ledger, so its fixed-role trigger is never met.
async fn apply_role(
    tx: &mut Transaction<'static, Postgres>,
    row: &Row,
    detail: &serde_json::Value,
) -> Result<bool, String> {
    let key = field(detail, "role")?;
    let changed = sqlx::query!(
        "DELETE FROM roles WHERE project_id = $1 AND key = $2",
        row.project_id,
        key
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    if changed.rows_affected() > 0 {
        replayed_audit(tx, row, key).await?;
    }
    Ok(changed.rows_affected() > 0)
}

/// The asset's terminal `deleted` state, landed without its objects: a restored backup can
/// resurrect the row, never the bytes the worker removed.
async fn apply_asset(
    tx: &mut Transaction<'static, Postgres>,
    row: &Row,
    detail: &serde_json::Value,
) -> Result<bool, String> {
    let asset = uuid_field(detail, "asset")?;
    let environment = row
        .environment_id
        .ok_or_else(|| "an asset row has no environment".to_owned())?;
    let changed = sqlx::query!(
        "UPDATE assets SET state = 'deleted'
         WHERE project_id = $1 AND environment_id = $2 AND id = $3 AND state <> 'deleted'",
        row.project_id,
        environment,
        asset
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    if changed.rows_affected() > 0 {
        replayed_audit(tx, row, &asset.to_string()).await?;
    }
    Ok(changed.rows_affected() > 0)
}

/// `colophon_delete_project` again — the same ordered removal a live deletion performs. The
/// transaction's context names the project and the restorer identity.
async fn apply_project(tx: &mut Transaction<'static, Postgres>, row: &Row) -> Result<bool, String> {
    let outcome = sqlx::query_scalar!(
        r#"SELECT outcome AS "outcome!" FROM colophon_delete_project($1)"#,
        row.project_id
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(short)?;
    match outcome.as_str() {
        "deleted" => Ok(true),
        "not_found" => Ok(false),
        _ => Err("colophon_delete_project gave an unknown outcome".to_owned()),
    }
}

/// The honest mark the replay leaves in the project's audit log: `ledger.replayed`, by the
/// restorer, naming the row. The original event — written then — died with the backup.
async fn replayed_audit(
    tx: &mut Transaction<'static, Postgres>,
    row: &Row,
    subject: &str,
) -> Result<(), String> {
    sqlx::query!(
        "INSERT INTO audit_events (project_id, id, actor_id, action, subject, detail)
         VALUES ($1, $2, $3, 'ledger.replayed', $4, $5::text::jsonb)",
        row.project_id,
        Uuid::now_v7(),
        RESTORER,
        subject,
        serde_json::json!({"ledger_id": row.id, "kind": row.kind.as_str()}).to_string()
    )
    .execute(&mut **tx)
    .await
    .map_err(short)?;
    Ok(())
}
