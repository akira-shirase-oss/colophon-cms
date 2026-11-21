//! Migrating as a role that is not a superuser (D-234): Neon's database owner, for one, where no
//! role is a superuser.
//!
//! Four things a superuser never notices stand in the way, and this module handles each, only
//! for a migrator that isn't one (a superuser runs [`crate::MIGRATOR`] unchanged):
//!
//! - **Pinned custom settings.** Migrations 0011, 0012, 0051 and 0082 attach
//!   `SET app.project_id = ''` (and 0082 `SET app.audience_role = ''`) to a function. PostgreSQL
//!   allows that for an undeclared setting only to a superuser or a role granted SET on the
//!   parameter, and Neon allows neither. Applied migrations never change, so those four run from
//!   substitutes (`migrations-nonsuperuser/`) that write the same guarantee in the function body,
//!   while the database records each original's checksum: to sqlx and to
//!   `admin migrate-status` the two kinds of installation look the same.
//! - **Membership.** On PostgreSQL 16 and later a role with CREATEROLE that makes another role
//!   gets only ADMIN on it, but handing an object to a role needs SET on it, and replacing an
//!   object a role owns needs its privileges (INHERIT). [`ensure_memberships`] grants this login
//!   both on every `colophon_*` role it has no usable membership in yet, and leaves an existing
//!   one's INHERIT as it is: migration 0048 deliberately makes `colophon_deleter`'s INHERIT FALSE
//!   (the ledger replay runs by `SET ROLE`). A run lifts INHERIT where it is off and puts it back
//!   afterwards; roles a migration makes (`colophon_migrator`, 0041) get SET and INHERIT through
//!   `createrole_self_grant`.
//! - **Schema rights of owners.** Giving an object in `public` to a role needs that role to hold
//!   CREATE on `public` (a superuser skips the check). The owner roles get it for the run only,
//!   and lose it afterwards whether the run succeeded or not: `colophon_app` can `SET ROLE` to
//!   `colophon_delivery` and `colophon_worker`, so a lasting grant would let a compromised
//!   application create objects in the schema every query searches.
//! - **Role passwords** (`login_roles`): Neon refuses a SCRAM verifier and takes only the
//!   password itself.

use std::sync::LazyLock;

use sqlx::{
    AssertSqlSafe, PgConnection, SqlStr,
    migrate::{Migration, Migrator},
};

use crate::{DbError, MIGRATOR};

/// The migrations a non-superuser runs from a substitute, by version: the only ones that attach
/// a custom setting to a function. Pinned by a test; a later migration writes the explicit form
/// instead (a test refuses a new `SET app.` clause).
pub const SUBSTITUTES: [(i64, &str); 4] = [
    (
        11,
        include_str!("../migrations-nonsuperuser/0011_delivery_environment.sql"),
    ),
    (
        12,
        include_str!("../migrations-nonsuperuser/0012_unpublish.sql"),
    ),
    (
        51,
        include_str!("../migrations-nonsuperuser/0051_asset_versions.sql"),
    ),
    (
        82,
        include_str!("../migrations-nonsuperuser/0082_private_media.sql"),
    ),
];

/// [`crate::MIGRATOR`] with the four substitutes' SQL in place of their originals, and every
/// migration's version, description and checksum unchanged.
pub static NON_SUPERUSER_MIGRATOR: LazyLock<Migrator> = LazyLock::new(|| {
    let migrations: Vec<Migration> = MIGRATOR
        .iter()
        .map(|migration| {
            let mut migration = migration.clone();
            if let Some(&(_, sql)) = SUBSTITUTES
                .iter()
                .find(|(version, _)| *version == migration.version)
            {
                // The checksum stays the original's: it identifies the migration, not this text.
                migration.sql = SqlStr::from_static(sql);
            }
            migration
        })
        .collect();
    // Field for field what `migrate!` gives MIGRATOR (sqlx's macro fields, pinned by Cargo.lock).
    Migrator {
        migrations: std::borrow::Cow::Owned(migrations),
        ignore_missing: MIGRATOR.ignore_missing,
        locking: MIGRATOR.locking,
        no_tx: MIGRATOR.no_tx,
        table_name: MIGRATOR.table_name.clone(),
        create_schemas: MIGRATOR.create_schemas.clone(),
    }
});

/// Whether the connection's role is a superuser.
///
/// # Errors
///
/// The query failed.
pub async fn is_superuser(conn: &mut PgConnection) -> Result<bool, DbError> {
    sqlx::query_scalar("SELECT rolsuper FROM pg_roles WHERE rolname = current_user")
        .fetch_one(conn)
        .await
        .map_err(DbError::from_query)
}

/// Grants this login SET and INHERIT on every `colophon_*` role (not itself) in which it has no
/// membership with SET yet, so it can hand objects to them and replace theirs. An existing
/// membership with SET is left exactly as it is, INHERIT included (0048 turns `colophon_deleter`'s
/// off on purpose). Idempotent. Needs ADMIN on each, which the role that created them holds.
///
/// # Errors
///
/// The query failed, or this login may not grant one of them.
pub async fn ensure_memberships(conn: &mut PgConnection) -> Result<(), DbError> {
    let missing: Vec<String> = sqlx::query_scalar(
        r"SELECT format('GRANT %I TO CURRENT_USER WITH SET TRUE, INHERIT TRUE', r.rolname)
          FROM pg_roles r
          WHERE r.rolname LIKE 'colophon\_%' AND r.rolname <> current_user
            AND NOT EXISTS (
                SELECT FROM pg_auth_members m
                WHERE m.roleid = r.oid AND m.set_option
                  AND m.member = (SELECT oid FROM pg_roles WHERE rolname = current_user))
          ORDER BY r.rolname",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(DbError::from_query)?;
    execute_all(conn, missing).await
}

/// Runs statements the server built with `format('%I')` from the catalog's own role names.
async fn execute_all(conn: &mut PgConnection, statements: Vec<String>) -> Result<(), DbError> {
    for statement in statements {
        sqlx::raw_sql(AssertSqlSafe(statement))
            .execute(&mut *conn)
            .await
            .map_err(DbError::from_query)?;
    }
    Ok(())
}

/// The `colophon_*` roles this login is a member of with SET but without INHERIT: a run lifts
/// INHERIT on them, so it can replace what they own, and puts it back after.
async fn members_without_inherit(conn: &mut PgConnection) -> Result<Vec<String>, DbError> {
    sqlx::query_scalar(
        r"SELECT r.rolname FROM pg_roles r
          WHERE r.rolname LIKE 'colophon\_%' AND r.rolname <> current_user
            AND EXISTS (
                SELECT FROM pg_auth_members m
                WHERE m.roleid = r.oid AND m.set_option
                  AND m.member = (SELECT oid FROM pg_roles WHERE rolname = current_user))
            AND NOT EXISTS (
                SELECT FROM pg_auth_members m
                WHERE m.roleid = r.oid AND m.inherit_option
                  AND m.member = (SELECT oid FROM pg_roles WHERE rolname = current_user))
          ORDER BY r.rolname",
    )
    .fetch_all(conn)
    .await
    .map_err(DbError::from_query)
}

/// `GRANT <role> TO CURRENT_USER WITH INHERIT <inherit>` for each of `roles`, quoted by the server.
async fn set_inherit(
    conn: &mut PgConnection,
    roles: &[String],
    inherit: bool,
) -> Result<(), DbError> {
    if roles.is_empty() {
        return Ok(());
    }
    let template = if inherit {
        "GRANT %I TO CURRENT_USER WITH INHERIT TRUE"
    } else {
        "GRANT %I TO CURRENT_USER WITH INHERIT FALSE"
    };
    let statements: Vec<String> =
        sqlx::query_scalar("SELECT format($1, role) FROM unnest($2::text[]) AS role ORDER BY role")
            .bind(template)
            .bind(roles)
            .fetch_all(&mut *conn)
            .await
            .map_err(DbError::from_query)?;
    execute_all(conn, statements).await
}

/// The `colophon_*` roles that log in nowhere and hold no CREATE on `public` of their own: those
/// the run grants it to.
async fn owners_without_create(conn: &mut PgConnection) -> Result<Vec<String>, DbError> {
    sqlx::query_scalar(
        r"SELECT r.rolname FROM pg_roles r
          WHERE r.rolname LIKE 'colophon\_%' AND NOT r.rolcanlogin
            AND NOT EXISTS (
                SELECT FROM pg_namespace n,
                     aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) a
                WHERE n.nspname = 'public' AND a.grantee = r.oid AND a.privilege_type = 'CREATE')
          ORDER BY r.rolname",
    )
    .fetch_all(conn)
    .await
    .map_err(DbError::from_query)
}

/// Every `colophon_*` role now.
async fn colophon_roles(conn: &mut PgConnection) -> Result<Vec<String>, DbError> {
    sqlx::query_scalar(r"SELECT rolname FROM pg_roles WHERE rolname LIKE 'colophon\_%' ORDER BY 1")
        .fetch_all(conn)
        .await
        .map_err(DbError::from_query)
}

/// `GRANT`s or `REVOKE`s CREATE on `public` for `roles`, quoted by the server.
async fn schema_create(
    conn: &mut PgConnection,
    grant: bool,
    roles: &[String],
) -> Result<(), DbError> {
    if roles.is_empty() {
        return Ok(());
    }
    let template = if grant {
        "GRANT CREATE ON SCHEMA public TO %s"
    } else {
        "REVOKE CREATE ON SCHEMA public FROM %s"
    };
    let statement: String = sqlx::query_scalar(
        "SELECT format($1, string_agg(quote_ident(role), ', ' ORDER BY role)) FROM unnest($2::text[]) AS role",
    )
    .bind(template)
    .bind(roles)
    .fetch_one(&mut *conn)
    .await
    .map_err(DbError::from_query)?;
    sqlx::raw_sql(AssertSqlSafe(statement))
        .execute(conn)
        .await
        .map_err(DbError::from_query)?;
    Ok(())
}

/// Advisory lock key ("MG", 0): one non-superuser run at a time, since the run's grants are
/// undone at its end and another run's would be undone with them.
const RUN_LOCK: (i32, i32) = (0x4d47, 0);

/// Applies pending migrations as a non-superuser: [`NON_SUPERUSER_MIGRATOR`], with the
/// memberships and the run's CREATE grants above. The grants are revoked whether the run
/// succeeded or failed (a crash leaves them until the next run revokes them).
///
/// # Errors
///
/// A grant or the run failed; the run's own error comes first.
pub async fn migrate(conn: &mut PgConnection) -> Result<(), DbError> {
    sqlx::query("SELECT pg_advisory_lock($1, $2)")
        .bind(RUN_LOCK.0)
        .bind(RUN_LOCK.1)
        .execute(&mut *conn)
        .await
        .map_err(DbError::from_query)?;
    let ran = run(conn).await;
    let unlocked = sqlx::query("SELECT pg_advisory_unlock($1, $2)")
        .bind(RUN_LOCK.0)
        .bind(RUN_LOCK.1)
        .execute(&mut *conn)
        .await
        .map_err(DbError::from_query);
    ran?;
    unlocked?;
    Ok(())
}

async fn run(conn: &mut PgConnection) -> Result<(), DbError> {
    // Roles a migration makes are granted to this login with SET and INHERIT, as init-roles'
    // are by ensure_memberships. Session-level, and reset below.
    sqlx::query("SET createrole_self_grant = 'set, inherit'")
        .execute(&mut *conn)
        .await
        .map_err(DbError::from_query)?;
    let mut run = Run::default();
    let prepared = prepare(conn, &mut run).await;
    let migrated = match prepared {
        Ok(()) => NON_SUPERUSER_MIGRATOR
            .run(&mut *conn)
            .await
            .map_err(DbError::Migrate),
        Err(error) => Err(error),
    };
    // Undo the run's grants whether it succeeded or not (and whatever of them prepare made).
    let undone = undo(conn, run).await;
    let reset = reset(conn).await;
    migrated?;
    undone?;
    reset
}

/// What a run changed for itself, to undo at its end.
#[derive(Default)]
struct Run {
    /// Every `colophon_*` role before the run.
    roles_before: Vec<String>,
    /// The roles given CREATE on `public` for the run.
    created: Vec<String>,
    /// The roles whose INHERIT the run lifted.
    inherited: Vec<String>,
}

/// Memberships, then the run's own grants, each recorded in `run` as soon as it is made.
async fn prepare(conn: &mut PgConnection, run: &mut Run) -> Result<(), DbError> {
    ensure_memberships(conn).await?;
    run.roles_before = colophon_roles(conn).await?;
    let inherit = members_without_inherit(conn).await?;
    set_inherit(conn, &inherit, true).await?;
    run.inherited = inherit;
    let create = owners_without_create(conn).await?;
    schema_create(conn, true, &create).await?;
    run.created = create;
    Ok(())
}

/// Revokes CREATE from the roles the run gave it to and from any role the run made (never from a
/// role that held it before), and puts INHERIT back where the run lifted it.
async fn undo(conn: &mut PgConnection, run: Run) -> Result<(), DbError> {
    let now = colophon_roles(conn).await?;
    let mut revoke = run.created;
    revoke.extend(
        now.into_iter()
            .filter(|role| !run.roles_before.contains(role)),
    );
    schema_create(conn, false, &revoke).await?;
    set_inherit(conn, &run.inherited, false).await
}

async fn reset(conn: &mut PgConnection) -> Result<(), DbError> {
    sqlx::query("RESET createrole_self_grant")
        .execute(conn)
        .await
        .map(|_| ())
        .map_err(DbError::from_query)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The substitute set is exactly these four, each with its original in `migrations/`.
    #[test]
    fn the_substitutes_are_exactly_the_four_that_pin_a_setting() {
        let versions: Vec<i64> = SUBSTITUTES.iter().map(|(version, _)| *version).collect();
        assert_eq!(versions, [11, 12, 51, 82]);
        // And no other migration up to them pins one: the four are the whole set.
        let pinned: Vec<i64> = MIGRATOR
            .iter()
            .filter(|migration| pins_a_custom_setting(migration.sql.as_str()))
            .map(|migration| migration.version)
            .collect();
        assert_eq!(pinned, versions, "the migrations that pin an app.* setting");
    }

    /// A line that attaches a custom `app.*` setting to a function (or a role or database).
    fn pins_a_custom_setting(sql: &str) -> bool {
        sql.lines().any(|line| {
            let line = line.trim_start().to_ascii_lowercase();
            !line.starts_with("--")
                && (line.starts_with("set app.")
                    || (line.starts_with("alter ") && line.contains(" set app.")))
        })
    }

    /// No migration after the four pins a custom setting: new code writes the explicit form,
    /// which works for every migrator.
    #[test]
    fn no_later_migration_pins_a_custom_setting() {
        let later: Vec<i64> = MIGRATOR
            .iter()
            .filter(|migration| {
                migration.version > 82 && pins_a_custom_setting(migration.sql.as_str())
            })
            .map(|migration| migration.version)
            .collect();
        assert!(
            later.is_empty(),
            "migrations {later:?} attach a SET app.* clause, which a non-superuser (Neon) can't apply; \
             capture, set_config and restore in the body instead (migrations-nonsuperuser/ shows how)"
        );
    }

    /// Lines a substitute may add: comments, blanks, the DECLARE block of captured caller values,
    /// and `set_config` calls on an `app.*` setting.
    fn may_be_added(line: &str) -> bool {
        let line = line.trim();
        line.is_empty()
            || line.starts_with("--")
            || line == "DECLARE"
            || (line.starts_with("caller_") && line.contains(" text := current_setting('app."))
            || line.starts_with("PERFORM set_config('app.")
    }

    /// Each substitute is its original minus the `SET app.` clause lines, in order, plus only
    /// lines [`may_be_added`] allows: the function bodies' SQL is the original's.
    #[test]
    fn each_substitute_differs_from_its_original_only_in_the_setting_lines() {
        for (version, substitute) in SUBSTITUTES {
            let original = MIGRATOR
                .iter()
                .find(|migration| migration.version == version)
                .expect("invariant: every substitute has an original");
            let mut expected = original
                .sql
                .as_str()
                .lines()
                .filter(|line| !line.trim_start().starts_with("SET app."))
                .peekable();
            for line in substitute.lines() {
                if expected.peek() == Some(&line) {
                    expected.next();
                } else {
                    assert!(
                        may_be_added(line),
                        "migration {version}'s substitute adds a line it may not: {line:?}"
                    );
                }
            }
            let left: Vec<&str> = expected.collect();
            assert!(
                left.is_empty(),
                "migration {version}'s substitute drops or changes original lines: {left:?}"
            );
            assert!(
                !substitute
                    .lines()
                    .any(|line| line.trim_start().starts_with("SET app.")),
                "migration {version}'s substitute still pins a setting"
            );
        }
    }

    /// Every function a substitute defines captures each setting it changes, and puts it back
    /// after its query; `media_object` also blanks the caller's audience before its query (D-229:
    /// what it calls published must not depend on who asks).
    #[test]
    fn each_substitute_function_restores_what_it_changes() {
        for (version, substitute) in SUBSTITUTES {
            for function in substitute.split("CREATE FUNCTION").skip(1) {
                let body = function.split("$$").nth(1).expect("invariant: a $$ body");
                let query = body
                    .find("RETURN QUERY")
                    .expect("invariant: one RETURN QUERY");
                let (before, after) = body.split_at(query);
                for (setting, variable) in [
                    ("app.project_id", "caller_project_id"),
                    ("app.audience_role", "caller_audience_role"),
                ] {
                    if !body.contains(&format!("set_config('{setting}'")) {
                        continue;
                    }
                    assert!(
                        before.contains(&format!(
                            "{variable} text := current_setting('{setting}', true);"
                        )),
                        "migration {version}: {setting} isn't captured before it is set"
                    );
                    assert!(
                        after.contains(&format!(
                            "PERFORM set_config('{setting}', coalesce({variable}, ''), true);"
                        )),
                        "migration {version}: {setting} isn't restored after the query"
                    );
                }
                if body.contains("caller_audience_role") {
                    assert!(
                        before.contains("PERFORM set_config('app.audience_role', '', true);"),
                        "migration {version}: the caller's audience isn't blanked before the query"
                    );
                }
                // One RETURN QUERY, then only the restores: no other return path to miss.
                assert_eq!(
                    body.matches("RETURN").count(),
                    1,
                    "migration {version}: one return path"
                );
            }
        }
    }

    /// The migrator a non-superuser runs is MIGRATOR's, migration for migration, checksums
    /// included; only the four substitutes' SQL differs.
    #[test]
    fn the_non_superuser_migrator_records_the_original_checksums() {
        let original: Vec<&Migration> = MIGRATOR.iter().collect();
        let substituted: Vec<&Migration> = NON_SUPERUSER_MIGRATOR.iter().collect();
        assert_eq!(original.len(), substituted.len());
        for (a, b) in original.iter().zip(&substituted) {
            assert_eq!(a.version, b.version);
            assert_eq!(a.description, b.description);
            assert_eq!(a.checksum, b.checksum, "migration {}", a.version);
            assert_eq!(a.no_tx, b.no_tx);
            let replaced = SUBSTITUTES.iter().any(|(version, _)| *version == a.version);
            assert_eq!(
                a.sql.as_str() != b.sql.as_str(),
                replaced,
                "migration {}",
                a.version
            );
        }
        assert_eq!(MIGRATOR.locking, NON_SUPERUSER_MIGRATOR.locking);
        assert_eq!(MIGRATOR.table_name, NON_SUPERUSER_MIGRATOR.table_name);
    }
}
