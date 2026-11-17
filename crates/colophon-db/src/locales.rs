//! An environment's languages (migration 0065; D-171): the list with its default and
//! fallbacks, replaced whole under the policy version, and the check entry writes make against
//! it. Every function runs inside a [`ProjectTransaction`], so row-level security confines it
//! to the project; the caller has checked the role.

use colophon_domain::locale::Language;
use uuid::Uuid;

use crate::{DbError, scope::ProjectTransaction};

/// The most entries a `locale_in_use` refusal names.
pub const MAX_IN_THE_WAY: i64 = 20;

/// The environment's languages and the version that names this list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Policy {
    /// Bumped by every replace; 0 before the first.
    pub version: i64,
    /// In display order.
    pub languages: Vec<Language>,
}

/// An entry written in a language a replace would remove.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InUse {
    /// The entry.
    pub entry_id: Uuid,
    /// Its collection.
    pub collection: String,
    /// Its draft's title.
    pub title: String,
}

/// What a replace did. The refusals write nothing; the caller rolls back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Replaced {
    /// The list is the new one, under this version.
    Replaced {
        /// The new version.
        version: i64,
    },
    /// The list moved on since `expected`: this is its version now.
    Stale {
        /// The current version.
        current: i64,
    },
    /// Live entries are written in a language the new list doesn't have: the first such tag,
    /// how many carry it, and up to [`MAX_IN_THE_WAY`] of them.
    InUse {
        /// The tag, as the old list spelled it.
        tag: String,
        /// How many live entries carry it.
        total: i64,
        /// The first few.
        entries: Vec<InUse>,
    },
}

/// The transaction's environment's languages and policy version.
pub async fn policy(tx: &mut ProjectTransaction) -> Result<Policy, DbError> {
    let version = sqlx::query_scalar!(
        "SELECT locale_policy_version FROM environments WHERE project_id = $1 AND id = $2",
        tx.project,
        tx.environment
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?
    .unwrap_or(0);
    let rows = sqlx::query!(
        "SELECT tag, label, is_default, fallback FROM environment_locales
         WHERE project_id = $1 AND environment_id = $2
         ORDER BY position",
        tx.project,
        tx.environment
    )
    .fetch_all(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(Policy {
        version,
        languages: rows
            .into_iter()
            .map(|row| Language {
                tag: row.tag,
                label: row.label,
                default: row.is_default,
                fallback: row.fallback,
            })
            .collect(),
    })
}

/// Replaces the environment's languages with `languages` (already checked by
/// `colophon_domain::locale::check_policy`), in their order, if the list is still at
/// `expected` — the compare-and-set on `environments.locale_policy_version` is the lock: a
/// concurrent replace waits on the row and then finds the version moved. A non-empty list must
/// list every tag a live entry carries, in that spelling; an empty one returns to free tags.
pub async fn replace(
    tx: &mut ProjectTransaction,
    expected: i64,
    languages: &[Language],
) -> Result<Replaced, DbError> {
    let bumped = sqlx::query_scalar!(
        "UPDATE environments SET locale_policy_version = locale_policy_version + 1
         WHERE project_id = $1 AND id = $2 AND locale_policy_version = $3
         RETURNING locale_policy_version",
        tx.project,
        tx.environment,
        expected
    )
    .fetch_optional(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    let Some(version) = bumped else {
        let current = policy(tx).await?.version;
        return Ok(Replaced::Stale { current });
    };
    // Every tag live entries carry must stay listed, spelled as the list spells it — whether
    // the new list drops it or the environment's first list never had it — or those entries
    // would be in a language delivery can't ask for. An empty list goes back to free tags.
    if !languages.is_empty() {
        let kept: Vec<String> = languages
            .iter()
            .map(|language| language.tag.clone())
            .collect();
        let uncovered = sqlx::query!(
            r#"SELECT locale AS "tag!", count(*) AS "total!" FROM entries
               WHERE project_id = $1 AND environment_id = $2 AND deleted_at IS NULL
                 AND locale IS NOT NULL AND NOT (locale = ANY ($3::text[]))
               GROUP BY locale
               ORDER BY locale
               LIMIT 1"#,
            tx.project,
            tx.environment,
            &kept
        )
        .fetch_optional(&mut *tx.transaction)
        .await
        .map_err(DbError::from_query)?;
        if let Some(uncovered) = uncovered {
            let entries = sqlx::query_as!(
                InUse,
                "SELECT id AS entry_id, collection, draft_title AS title FROM entries
                 WHERE project_id = $1 AND environment_id = $2 AND deleted_at IS NULL
                   AND locale = $3
                 ORDER BY id DESC
                 LIMIT $4",
                tx.project,
                tx.environment,
                uncovered.tag,
                MAX_IN_THE_WAY
            )
            .fetch_all(&mut *tx.transaction)
            .await
            .map_err(DbError::from_query)?;
            return Ok(Replaced::InUse {
                tag: uncovered.tag,
                total: uncovered.total,
                entries,
            });
        }
    }
    sqlx::query!(
        "DELETE FROM environment_locales WHERE project_id = $1 AND environment_id = $2",
        tx.project,
        tx.environment
    )
    .execute(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    for (position, language) in languages.iter().enumerate() {
        let position = i16::try_from(position)
            .map_err(|_| DbError::Corrupt("a language list longer than its CHECK allows"))?;
        sqlx::query!(
            "INSERT INTO environment_locales
               (project_id, environment_id, tag, label, position, is_default, fallback)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
            tx.project,
            tx.environment,
            language.tag,
            language.label,
            position,
            language.default,
            &language.fallback
        )
        .execute(&mut *tx.transaction)
        .await
        .map_err(DbError::from_write)?;
    }
    Ok(Replaced::Replaced { version })
}

/// What an entry write's `?locale=` meets (D-171): `None` when the environment lists no
/// languages (any well-formed tag goes, as before); otherwise the listed spelling of `tag`,
/// or `Some(None)` when the list doesn't have it.
pub async fn configured(
    tx: &mut ProjectTransaction,
    tag: &str,
) -> Result<Option<Option<String>>, DbError> {
    let row = sqlx::query!(
        r#"SELECT EXISTS (SELECT 1 FROM environment_locales
                          WHERE project_id = $1 AND environment_id = $2) AS "listed!",
                  (SELECT l.tag FROM environment_locales l
                   WHERE l.project_id = $1 AND l.environment_id = $2
                     AND lower(l.tag) = lower($3)) AS spelled"#,
        tx.project,
        tx.environment,
        tag
    )
    .fetch_one(&mut *tx.transaction)
    .await
    .map_err(DbError::from_query)?;
    Ok(row.listed.then_some(row.spelled))
}
