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
//!   `admin migrate-status` the two kinds of installation look the same (each substitute's
//!   functions carry a `COMMENT` saying what they are).
//! - **Membership.** On PostgreSQL 16 and later a role with CREATEROLE that makes another role
//!   gets only ADMIN on it, but handing an object to a role needs SET on it, and replacing an
//!   object a role owns needs its privileges (INHERIT). [`ensure_memberships`] grants this login
//!   both on every `colophon_*` role it has no usable membership in yet, and leaves an existing
//!   one's INHERIT as it is: migration 0048 deliberately makes `colophon_deleter`'s INHERIT FALSE
//!   (the ledger replay runs by `SET ROLE`).
//! - **Schema rights of owners.** Giving an object in `public` to a role needs that role to hold
//!   CREATE on `public` (a superuser skips the check).
//! - **Role passwords** (`login_roles`): Neon refuses a SCRAM verifier and takes only the
//!   password itself.
//!
//! The grants the last two need exist **only inside each migration's own transaction**: every
//! migration runs as [`PROLOGUE`], its SQL, [`EPILOGUE`]. The prologue grants CREATE on `public`
//! to the owner roles and lifts INHERIT where this login has SET without it; the epilogue revokes
//! CREATE on `public` from every `colophon_*` role and puts INHERIT back where the prologue lifted
//! it. Uncommitted, the grants are invisible to every other session (`colophon_app`, which can
//! `SET ROLE` to `colophon_delivery` and `colophon_worker`, can never use them), and a run that
//! dies mid-migration takes them with its rollback. [`SWEEP`] runs at the start and end of every
//! run and in `init-roles` as a backstop, and [`grant_leftovers`] reports anything that got past
//! all of it (`admin migrate-status`, `smoke.sh`).

use std::{sync::LazyLock, time::Duration};

use sqlx::{
    AssertSqlSafe, PgConnection, SqlSafeStr,
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

/// Runs first in every migration's transaction, for a non-superuser: CREATE on `public` for each
/// `colophon_*` role that logs in nowhere and lacks it (so objects can be handed to it), INHERIT
/// lifted on each `colophon_*` role this login has SET on but doesn't inherit (so what it owns can
/// be replaced), and roles the migration makes granted to this login with SET and INHERIT. What it
/// lifted is kept in a transaction-local setting for [`EPILOGUE`]: `role:kept` when this login's
/// own membership row existed (put back to INHERIT FALSE), `role:new` when the GRANT made it
/// (revoked). Uncommitted, none of it is visible to another session.
pub const PROLOGUE: &str = r"-- D-234 (colophon_db::nonsuperuser::PROLOGUE): this migration's own grants, inside its transaction.
DO $colophon_d234$
DECLARE
    r record;
    lifted text := '';
BEGIN
    PERFORM pg_catalog.set_config('createrole_self_grant', 'set, inherit', true);
    FOR r IN
        SELECT ro.rolname FROM pg_catalog.pg_roles ro
        WHERE ro.rolname LIKE 'colophon\_%' AND NOT ro.rolcanlogin
          AND NOT EXISTS (
              SELECT FROM pg_catalog.pg_namespace n,
                   pg_catalog.aclexplode(coalesce(n.nspacl, pg_catalog.acldefault('n', n.nspowner))) a
              WHERE n.nspname = 'public' AND a.grantee = ro.oid AND a.privilege_type = 'CREATE')
        ORDER BY 1
    LOOP
        EXECUTE pg_catalog.format('GRANT CREATE ON SCHEMA public TO %I', r.rolname);
    END LOOP;
    FOR r IN
        SELECT ro.rolname,
               EXISTS (SELECT FROM pg_catalog.pg_auth_members m
                       WHERE m.roleid = ro.oid AND m.member = me.oid AND m.grantor = me.oid) AS own_row
        FROM pg_catalog.pg_roles ro, (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = current_user) me
        WHERE ro.rolname LIKE 'colophon\_%' AND ro.oid <> me.oid
          AND EXISTS (SELECT FROM pg_catalog.pg_auth_members m
                      WHERE m.roleid = ro.oid AND m.member = me.oid AND m.set_option)
          AND NOT EXISTS (SELECT FROM pg_catalog.pg_auth_members m
                          WHERE m.roleid = ro.oid AND m.member = me.oid AND m.inherit_option)
        ORDER BY 1
    LOOP
        EXECUTE pg_catalog.format('GRANT %I TO CURRENT_USER WITH INHERIT TRUE', r.rolname);
        lifted := lifted || r.rolname || CASE WHEN r.own_row THEN ':kept,' ELSE ':new,' END;
    END LOOP;
    PERFORM pg_catalog.set_config('colophon_d234.lifted', lifted, true);
END
$colophon_d234$;
";

/// Runs last in every migration's transaction, for a non-superuser: CREATE on `public` revoked
/// from every `colophon_*` role that holds it (none may outside a migration; no migration grants
/// it), and INHERIT put back exactly where [`PROLOGUE`] lifted it. A migration that left its role
/// switched (`SET LOCAL ROLE` without `RESET ROLE`) is refused here rather than cleaned up as the
/// wrong role.
pub const EPILOGUE: &str = r"-- D-234 (colophon_db::nonsuperuser::EPILOGUE): this migration's own grants, taken back before it commits.
DO $colophon_d234$
DECLARE
    r record;
    item text;
    role_name text;
BEGIN
    IF current_user <> session_user THEN
        RAISE EXCEPTION 'D-234: this migration ended as role %, not %: it must RESET ROLE', current_user, session_user;
    END IF;
    FOR r IN
        SELECT ro.rolname FROM pg_catalog.pg_roles ro
        WHERE ro.rolname LIKE 'colophon\_%'
          AND EXISTS (
              SELECT FROM pg_catalog.pg_namespace n,
                   pg_catalog.aclexplode(coalesce(n.nspacl, pg_catalog.acldefault('n', n.nspowner))) a
              WHERE n.nspname = 'public' AND a.grantee = ro.oid AND a.privilege_type = 'CREATE')
        ORDER BY 1
    LOOP
        EXECUTE pg_catalog.format('REVOKE CREATE ON SCHEMA public FROM %I', r.rolname);
    END LOOP;
    FOREACH item IN ARRAY pg_catalog.string_to_array(coalesce(pg_catalog.current_setting('colophon_d234.lifted', true), ''), ',')
    LOOP
        CONTINUE WHEN item = '';
        role_name := pg_catalog.split_part(item, ':', 1);
        CONTINUE WHEN NOT EXISTS (
            SELECT FROM pg_catalog.pg_auth_members m
            WHERE m.roleid = pg_catalog.to_regrole(pg_catalog.quote_ident(role_name))
              AND m.member = (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = current_user)
              AND m.grantor = (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = current_user));
        IF pg_catalog.split_part(item, ':', 2) = 'kept' THEN
            EXECUTE pg_catalog.format('GRANT %I TO CURRENT_USER WITH INHERIT FALSE', role_name);
        ELSE
            EXECUTE pg_catalog.format('REVOKE %I FROM CURRENT_USER', role_name);
        END IF;
    END LOOP;
    PERFORM pg_catalog.set_config('colophon_d234.lifted', '', true);
END
$colophon_d234$;
";

/// The backstop, at the start and end of every non-superuser run and in `init-roles`: CREATE on
/// `public` revoked from every `colophon_*` role that holds it, and, once migration 0048 is
/// applied, this login's own `colophon_deleter` membership back to INHERIT FALSE, as 0048 defines
/// it. It undoes what an older build's crash, or a grant by hand, left behind.
pub const SWEEP: &str = r"DO $colophon_d234$
DECLARE
    r record;
BEGIN
    FOR r IN
        SELECT ro.rolname FROM pg_catalog.pg_roles ro
        WHERE ro.rolname LIKE 'colophon\_%'
          AND EXISTS (
              SELECT FROM pg_catalog.pg_namespace n,
                   pg_catalog.aclexplode(coalesce(n.nspacl, pg_catalog.acldefault('n', n.nspowner))) a
              WHERE n.nspname = 'public' AND a.grantee = ro.oid AND a.privilege_type = 'CREATE')
        ORDER BY 1
    LOOP
        EXECUTE pg_catalog.format('REVOKE CREATE ON SCHEMA public FROM %I', r.rolname);
    END LOOP;
    IF pg_catalog.to_regclass('public._sqlx_migrations') IS NOT NULL
       AND pg_catalog.to_regrole('colophon_deleter') IS NOT NULL THEN
        IF EXISTS (SELECT FROM public._sqlx_migrations WHERE version = 48 AND success)
           AND EXISTS (SELECT FROM pg_catalog.pg_auth_members m
                       WHERE m.roleid = 'colophon_deleter'::pg_catalog.regrole
                         AND m.member = (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = current_user)
                         AND m.grantor = (SELECT oid FROM pg_catalog.pg_roles WHERE rolname = current_user)
                         AND m.inherit_option) THEN
            GRANT colophon_deleter TO CURRENT_USER WITH INHERIT FALSE;
        END IF;
    END IF;
END
$colophon_d234$;
";

/// `sql` as a non-superuser runs it: [`PROLOGUE`], the SQL, [`EPILOGUE`], in one transaction.
pub fn wrapped(sql: &str) -> String {
    format!("{PROLOGUE}\n{sql}\n{EPILOGUE}")
}

/// [`crate::MIGRATOR`] with the four substitutes' SQL in place of their originals, every
/// migration wrapped in [`PROLOGUE`] and [`EPILOGUE`], and every migration's version, description
/// and checksum unchanged. There is no no-transaction migration (a test pins that): one would need
/// its grants committed, which this design exists to avoid.
pub static NON_SUPERUSER_MIGRATOR: LazyLock<Migrator> = LazyLock::new(|| {
    let migrations: Vec<Migration> = MIGRATOR
        .iter()
        .map(|migration| {
            let mut migration = migration.clone();
            let base = SUBSTITUTES
                .iter()
                .find(|(version, _)| *version == migration.version)
                .map_or(migration.sql.as_str(), |&(_, sql)| sql);
            // The checksum stays the original's: it identifies the migration, not this text.
            migration.sql = AssertSqlSafe(wrapped(base)).into_sql_str();
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
/// off on purpose; [`SWEEP`] keeps it so). Idempotent. Needs ADMIN on each, which the role that
/// created them holds.
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
    for statement in missing {
        // Built by the server with format('%I') from the catalog's own role names.
        sqlx::raw_sql(AssertSqlSafe(statement))
            .execute(&mut *conn)
            .await
            .map_err(DbError::from_query)?;
    }
    Ok(())
}

/// Runs [`SWEEP`].
///
/// # Errors
///
/// The sweep failed (this login may not revoke or grant one of the roles).
pub async fn sweep(conn: &mut PgConnection) -> Result<(), DbError> {
    sqlx::raw_sql(SWEEP)
        .execute(conn)
        .await
        .map(|_| ())
        .map_err(DbError::from_query)
}

/// What [`grant_leftovers`] found: grants that exist only inside a non-superuser migration,
/// outside one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantLeftovers {
    /// `colophon_*` roles holding CREATE on `public` (none should).
    pub create_on_public: Vec<String>,
    /// Whether this login inherits `colophon_deleter` though migration 0048 says it mustn't.
    pub deleter_inherited: bool,
}

impl GrantLeftovers {
    /// Nothing left behind.
    pub fn is_clean(&self) -> bool {
        self.create_on_public.is_empty() && !self.deleter_inherited
    }
}

/// The grants that should exist only inside a migration's transaction, found outside one: for
/// `admin migrate-status` (and through it `smoke.sh`). Read-only.
///
/// # Errors
///
/// The query failed.
pub async fn grant_leftovers(conn: &mut PgConnection) -> Result<GrantLeftovers, DbError> {
    let create_on_public: Vec<String> = sqlx::query_scalar(
        r"SELECT ro.rolname FROM pg_roles ro
          WHERE ro.rolname LIKE 'colophon\_%'
            AND EXISTS (
                SELECT FROM pg_namespace n, aclexplode(coalesce(n.nspacl, acldefault('n', n.nspowner))) a
                WHERE n.nspname = 'public' AND a.grantee = ro.oid AND a.privilege_type = 'CREATE')
          ORDER BY 1",
    )
    .fetch_all(&mut *conn)
    .await
    .map_err(DbError::from_query)?;
    let migrated_48: bool = sqlx::query_scalar(
        "SELECT to_regclass('public._sqlx_migrations') IS NOT NULL AND to_regrole('colophon_deleter') IS NOT NULL",
    )
    .fetch_one(&mut *conn)
    .await
    .map_err(DbError::from_query)?;
    let deleter_inherited = if migrated_48 {
        sqlx::query_scalar(
            r"SELECT EXISTS (SELECT FROM public._sqlx_migrations WHERE version = 48 AND success)
                 AND coalesce((SELECT bool_or(m.inherit_option) FROM pg_auth_members m
                               WHERE m.roleid = 'colophon_deleter'::regrole
                                 AND m.member = (SELECT oid FROM pg_roles WHERE rolname = current_user)), false)",
        )
        .fetch_one(&mut *conn)
        .await
        .map_err(DbError::from_query)?
    } else {
        false
    };
    Ok(GrantLeftovers {
        create_on_public,
        deleter_inherited,
    })
}

/// Advisory lock key ("MG", 0): one non-superuser run at a time.
const RUN_LOCK: (i32, i32) = (0x4d47, 0);
/// How long a run waits for another to finish before it gives up, saying so.
pub const RUN_LOCK_WAIT: Duration = Duration::from_secs(60);

/// Applies pending migrations as a non-superuser: [`ensure_memberships`], [`SWEEP`],
/// [`NON_SUPERUSER_MIGRATOR`] (each migration with its own grants inside its transaction), and
/// [`SWEEP`] again, under a run lock it waits [`RUN_LOCK_WAIT`] for at most.
///
/// # Errors
///
/// Another run held the lock throughout, a grant or the run failed; the run's own error first.
pub async fn migrate(conn: &mut PgConnection) -> Result<(), DbError> {
    let started = tokio::time::Instant::now();
    loop {
        let locked: bool = sqlx::query_scalar("SELECT pg_try_advisory_lock($1, $2)")
            .bind(RUN_LOCK.0)
            .bind(RUN_LOCK.1)
            .fetch_one(&mut *conn)
            .await
            .map_err(DbError::from_query)?;
        if locked {
            break;
        }
        if started.elapsed() >= RUN_LOCK_WAIT {
            return Err(DbError::RunLocked(RUN_LOCK_WAIT));
        }
        tokio::time::sleep(Duration::from_millis(500)).await;
    }
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
    ensure_memberships(conn).await?;
    sweep(conn).await?;
    let migrated = NON_SUPERUSER_MIGRATOR
        .run(&mut *conn)
        .await
        .map_err(DbError::Migrate);
    // The backstop whether the run succeeded or not (a failed migration's grants rolled back
    // with it already).
    let swept = sweep(conn).await;
    migrated?;
    swept
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

    /// SQL that attaches a custom `app.*` setting to a function, a role or a database: the word
    /// `set`, whitespace, then `app.` (or `"app.`), anywhere outside a `--` comment, in any case
    /// (`(?i)\bset\s+"?app\.`), which `ALTER ... SET app.x` and a one-line
    /// `SECURITY DEFINER SET app.x = ''` match as well. `set_config('app.` and `SET LOCAL app.`
    /// don't: they pin nothing.
    fn pins_a_custom_setting(sql: &str) -> bool {
        sql.lines().any(|line| {
            let code = line.split("--").next().unwrap_or("").to_ascii_lowercase();
            let bytes = code.as_bytes();
            code.match_indices("set").any(|(at, _)| {
                let starts_word =
                    at == 0 || !(bytes[at - 1].is_ascii_alphanumeric() || bytes[at - 1] == b'_');
                let rest = &code[at + 3..];
                let spaced = rest.trim_start();
                starts_word
                    && spaced.len() < rest.len()
                    && spaced.trim_start_matches('"').starts_with("app.")
            })
        })
    }

    /// The guard's own cases.
    #[test]
    fn the_guard_finds_every_form_of_a_pinned_setting() {
        for pinned in [
            "    SET app.project_id = ''",
            "    SECURITY DEFINER SET app.project_id = ''",
            "ALTER FUNCTION f() SET app.audience_role = ''",
            "ALTER ROLE colophon_app SET app.project_id TO ''",
            "    set\tapp.x = ''",
            "    SET  \"app.project_id\" = ''",
        ] {
            assert!(pins_a_custom_setting(pinned), "{pinned:?}");
        }
        for free in [
            "    PERFORM set_config('app.project_id', project::text, true);",
            "-- SET app.project_id = '' was here",
            "    x := 1; -- SET app.project_id",
            "    SET search_path = pg_catalog, pg_temp",
            "    reset app.project_id",
            "    offset app_count",
        ] {
            assert!(!pins_a_custom_setting(free), "{free:?}");
        }
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
            || (line.starts_with("COMMENT ON FUNCTION ")
                && line.ends_with(" IS 'D-234 non-superuser substitute: the caller''s app.* settings are captured and restored in the body instead of a SET clause (colophon_db::nonsuperuser)';"))
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
    /// included: each migration's SQL is exactly PROLOGUE, the original's (or, for the four, the
    /// substitute's) SQL, EPILOGUE.
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
            let base = SUBSTITUTES
                .iter()
                .find(|(version, _)| *version == a.version)
                .map_or(a.sql.as_str(), |&(_, sql)| sql);
            assert_eq!(
                b.sql.as_str(),
                format!("{PROLOGUE}\n{base}\n{EPILOGUE}"),
                "migration {}: not exactly prologue + its SQL + epilogue",
                a.version
            );
        }
        assert_eq!(MIGRATOR.locking, NON_SUPERUSER_MIGRATOR.locking);
        assert_eq!(MIGRATOR.table_name, NON_SUPERUSER_MIGRATOR.table_name);
    }

    /// Every migration runs in a transaction: the grants live and die with it. A no-transaction
    /// migration would need them committed (visible to `colophon_app`, left by a crash).
    #[test]
    fn no_migration_runs_outside_a_transaction() {
        let no_tx: Vec<i64> = MIGRATOR
            .iter()
            .filter(|migration| migration.no_tx)
            .map(|migration| migration.version)
            .collect();
        assert!(
            no_tx.is_empty(),
            "migrations {no_tx:?} run without a transaction: a non-superuser's PROLOGUE grants would commit (D-234); give them a run-level path with SWEEP"
        );
    }

    /// No migration grants a privilege on schema public itself: the epilogue revokes CREATE on it
    /// from every colophon role, so such a grant would be undone for a non-superuser only.
    #[test]
    fn no_migration_grants_on_schema_public() {
        let granting: Vec<i64> = MIGRATOR
            .iter()
            .filter(|migration| {
                migration.sql.as_str().lines().any(|line| {
                    let code = line.split("--").next().unwrap_or("").to_ascii_lowercase();
                    code.contains("on schema public")
                })
            })
            .map(|migration| migration.version)
            .collect();
        assert!(
            granting.is_empty(),
            "migrations {granting:?} grant on schema public"
        );
    }

    /// The prologue, the epilogue and the sweep never end the transaction they run in.
    #[test]
    fn the_wrappers_stay_inside_the_migration_transaction() {
        for (name, sql) in [
            ("PROLOGUE", PROLOGUE),
            ("EPILOGUE", EPILOGUE),
            ("SWEEP", SWEEP),
        ] {
            let upper: String = sql
                .lines()
                .map(|line| line.split("--").next().unwrap_or("").to_ascii_uppercase() + "\n")
                .collect();
            for word in [
                "COMMIT",
                "ROLLBACK",
                "BEGIN;",
                "START TRANSACTION",
                "SAVEPOINT",
            ] {
                assert!(!upper.contains(word), "{name} contains {word}");
            }
        }
    }
}
