//! The installation's database roles outside development (D-173): what
//! `deploy/postgres/roles.sql` creates for the local stack with its public password, created
//! here by `colophon admin init-roles` with passwords an installer generated.
//!
//! The two lists below are the single source; a unit test holds `roles.sql` to them, so the
//! development stack and an installation can never disagree on a role or a grant. Passwords
//! don't travel as text where the server allows it: each login role gets a SCRAM-SHA-256 verifier
//! computed here, so the password is not in the statement, in `pg_stat_activity`, or in a server
//! log that records a failed statement. Neon refuses a verifier ("Neon only supports being given
//! plaintext passwords"), and only on that refusal [`init`] runs again with the password itself
//! in the statement, over the connection's verify-full TLS (D-234).

use std::num::NonZeroU32;

use aws_lc_rs::{digest, hmac, pbkdf2, rand::SecureRandom as _};
use base64::Engine as _;
use sqlx::PgPool;

use crate::DbError;

/// Every role, with the attributes `CREATE ROLE` gives it (passwords aside). Order matters:
/// `colophon_app` is created `IN ROLE colophon_api`.
pub const ROLES: [(&str, &str); 9] = [
    ("colophon_api", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
    (
        "colophon_app",
        "LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE IN ROLE colophon_api",
    ),
    ("colophon_delivery", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
    ("colophon_worker", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
    ("colophon_deleter", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
    (
        "colophon_backup",
        "LOGIN REPLICATION NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE",
    ),
    ("colophon_retention", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
    ("colophon_accounts", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
    ("colophon_audience", "NOLOGIN NOSUPERUSER NOBYPASSRLS"),
];

/// The memberships between them; each is idempotent.
pub const GRANTS: [&str; 4] = [
    "GRANT colophon_api TO colophon_deleter WITH INHERIT TRUE",
    // Not inherited: colophon_app holds these privileges only after SET ROLE.
    "GRANT colophon_delivery TO colophon_app WITH INHERIT FALSE, SET TRUE",
    "GRANT colophon_worker TO colophon_app WITH INHERIT FALSE, SET TRUE",
    "GRANT colophon_audience TO colophon_app WITH INHERIT FALSE, SET TRUE",
];

/// The shortest password a login role takes: a generated one is far longer.
pub const MIN_PASSWORD_CHARS: usize = 16;

/// The local stack's public password (`deploy/postgres/roles.sql`), refused everywhere else.
const DEVELOPMENT_PASSWORD: &str = "colophon-dev";

/// PostgreSQL's own default for `scram_iterations`.
const SCRAM_ITERATIONS: u32 = 4096;

/// Why a password was refused before anything reached the database.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PasswordRefused {
    /// The development stack's public password.
    #[error("that is the development stack's public password; generate one")]
    Development,
    /// Too short to be a generated secret.
    #[error("it is shorter than {MIN_PASSWORD_CHARS} characters; generate a longer one")]
    TooShort,
}

/// Checks a login role's password; the error never repeats it.
pub fn check_password(password: &str) -> Result<(), PasswordRefused> {
    if password == DEVELOPMENT_PASSWORD {
        Err(PasswordRefused::Development)
    } else if password.chars().count() < MIN_PASSWORD_CHARS {
        Err(PasswordRefused::TooShort)
    } else {
        Ok(())
    }
}

/// The SCRAM-SHA-256 verifier PostgreSQL stores for `password` with `salt` (RFC 5802, RFC
/// 7677): `SCRAM-SHA-256$<iterations>:<salt>$<StoredKey>:<ServerKey>`, base64. Given as a
/// role's `PASSWORD`, PostgreSQL stores it as is.
pub fn scram_verifier(password: &str, salt: &[u8]) -> String {
    let iterations = NonZeroU32::new(SCRAM_ITERATIONS).unwrap_or(NonZeroU32::MIN);
    let mut salted = [0_u8; 32];
    pbkdf2::derive(
        pbkdf2::PBKDF2_HMAC_SHA256,
        iterations,
        salt,
        password.as_bytes(),
        &mut salted,
    );
    let key = hmac::Key::new(hmac::HMAC_SHA256, &salted);
    let client_key = hmac::sign(&key, b"Client Key");
    let stored_key = digest::digest(&digest::SHA256, client_key.as_ref());
    let server_key = hmac::sign(&key, b"Server Key");
    let b64 = base64::engine::general_purpose::STANDARD;
    format!(
        "SCRAM-SHA-256${}:{}${}:{}",
        iterations,
        b64.encode(salt),
        b64.encode(stored_key.as_ref()),
        b64.encode(server_key.as_ref())
    )
}

/// What [`init`] did with one role.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoleOutcome {
    /// The role.
    pub name: &'static str,
    /// What happened to it.
    pub action: RoleAction,
}

/// What happened to a role.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoleAction {
    /// It didn't exist; now it does.
    Created,
    /// It existed and was left as it was, password included: running again changes nothing.
    Existing,
    /// A login role no password was given for, so it wasn't created (the backup login).
    Skipped,
}

impl RoleAction {
    /// The word the CLI prints.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Created => "created",
            Self::Existing => "existing",
            Self::Skipped => "skipped",
        }
    }
}

/// Why [`init`] failed.
#[derive(Debug, thiserror::Error)]
pub enum InitError {
    /// A password was refused; nothing was changed.
    #[error("the {role} password was refused: {reason}")]
    Password {
        /// Whose.
        role: &'static str,
        /// Why.
        reason: PasswordRefused,
    },
    /// No secure random source for the salts.
    #[error("no secure random source for the password salts")]
    Random,
    /// The database refused or failed.
    #[error(transparent)]
    Db(#[from] DbError),
    /// The retry with the password as text (Neon, D-234) failed too. That statement held the
    /// password, and PostgreSQL logs a failed statement's text (`log_min_error_statement`), so it
    /// may be in the database server's log: the password must be replaced.
    #[error(
        "the roles couldn't be created with the password as text either; that statement may be in the database server's log with the password in it: rotate the application's password (a new app_password file, then init-roles again)"
    )]
    TextPasswordFailed(#[source] DbError),
}

/// Creates every missing role and grants the memberships, in one transaction on the
/// migrator's pool (a role that may create roles). Idempotent: existing roles are left exactly
/// as they are, passwords included, so a rerun with the same files is a no-op. `app_password`
/// becomes `colophon_app`'s; `backup_password`, when given, creates the `colophon_backup`
/// login (`scripts/backup.sh`), and without it that role is skipped.
pub async fn init(
    pool: &PgPool,
    app_password: &str,
    backup_password: Option<&str>,
) -> Result<Vec<RoleOutcome>, InitError> {
    match init_as(pool, app_password, backup_password, PasswordForm::Verifier).await {
        // The whole transaction rolled back with the refused statement; running again is safe,
        // as every run is (existing roles are left as they are).
        Err(InitError::Db(DbError::Query(error))) if refuses_verifier_error(&error) => {
            init_as(pool, app_password, backup_password, PasswordForm::Text)
                .await
                .map_err(|error| match error {
                    InitError::Db(error) => InitError::TextPasswordFailed(error),
                    other => other,
                })
        }
        outcome => outcome,
    }
}

/// How a login role's password reaches `CREATE ROLE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordForm {
    /// A SCRAM-SHA-256 verifier computed here: the password itself never leaves this process.
    Verifier,
    /// The password itself, as a string literal: only for a server that refuses verifiers
    /// (Neon), over the connection's verify-full TLS.
    Text,
}

/// Neon's control plane takes a role's password only as text; given a verifier it answers
/// `Received HTTP code 400 from control plane: {..."error":"Neon only supports being given
/// plaintext passwords"}` (seen 2026-11-21). Nothing else counts as that refusal.
pub fn refuses_verifier(message: &str) -> bool {
    message
        .to_ascii_lowercase()
        .contains("only supports being given plaintext passwords")
}

fn refuses_verifier_error(error: &sqlx::Error) -> bool {
    error
        .as_database_error()
        .is_some_and(|database| refuses_verifier(database.message()))
}

/// `password` as a single-quoted SQL literal, for a statement run with
/// `standard_conforming_strings` on (a backslash is then an ordinary character).
fn text_literal(password: &str) -> String {
    format!("'{}'", password.replace('\'', "''"))
}

/// [`init`] with the passwords in one [`PasswordForm`].
///
/// # Errors
///
/// As [`init`].
pub async fn init_as(
    pool: &PgPool,
    app_password: &str,
    backup_password: Option<&str>,
    form: PasswordForm,
) -> Result<Vec<RoleOutcome>, InitError> {
    check_password(app_password).map_err(|reason| InitError::Password {
        role: "colophon_app",
        reason,
    })?;
    if let Some(password) = backup_password {
        check_password(password).map_err(|reason| InitError::Password {
            role: "colophon_backup",
            reason,
        })?;
    }
    let random = aws_lc_rs::rand::SystemRandom::new();
    // The `PASSWORD` operand for each login role: a verifier literal (base64, digits, `$` and `:`
    // only: nothing that can end the literal) or the password's own quoted literal.
    let verifier = |password: &str| match form {
        PasswordForm::Verifier => {
            let mut salt = [0_u8; 16];
            random.fill(&mut salt).map_err(|_| InitError::Random)?;
            Ok::<_, InitError>(format!("'{}'", scram_verifier(password, &salt)))
        }
        PasswordForm::Text => Ok(text_literal(password)),
    };
    let app_verifier = verifier(app_password)?;
    let backup_verifier = backup_password.map(verifier).transpose()?;

    let mut tx = crate::begin_tx(pool).await.map_err(DbError::from_query)?;
    // Two installers at once queue here instead of racing on CREATE ROLE ("RO" — roles).
    sqlx::query("SELECT pg_advisory_xact_lock(0x524f, 0)")
        .execute(&mut *tx)
        .await
        .map_err(DbError::from_query)?;
    if form == PasswordForm::Text {
        // The literal above doubles quotes only, which is complete with this on.
        sqlx::query("SET LOCAL standard_conforming_strings = on")
            .execute(&mut *tx)
            .await
            .map_err(DbError::from_query)?;
    }
    let superuser = crate::nonsuperuser::is_superuser(&mut tx).await?;
    let mut outcomes = Vec::with_capacity(ROLES.len());
    for (name, attributes) in ROLES {
        let exists: bool =
            sqlx::query_scalar("SELECT EXISTS (SELECT FROM pg_roles WHERE rolname = $1)")
                .bind(name)
                .fetch_one(&mut *tx)
                .await
                .map_err(DbError::from_query)?;
        let password = match name {
            "colophon_app" => Some(app_verifier.as_str()),
            "colophon_backup" => match &backup_verifier {
                Some(verifier) => Some(verifier.as_str()),
                None if exists => None,
                None => {
                    outcomes.push(RoleOutcome {
                        name,
                        action: RoleAction::Skipped,
                    });
                    continue;
                }
            },
            _ => None,
        };
        if exists {
            outcomes.push(RoleOutcome {
                name,
                action: RoleAction::Existing,
            });
            continue;
        }
        let statement = match password {
            Some(operand) => format!("CREATE ROLE {name} {attributes} PASSWORD {operand}"),
            None => format!("CREATE ROLE {name} {attributes}"),
        };
        sqlx::raw_sql(sqlx::AssertSqlSafe(statement))
            .execute(&mut *tx)
            .await
            .map_err(DbError::from_query)?;
        outcomes.push(RoleOutcome {
            name,
            action: RoleAction::Created,
        });
    }
    for grant in GRANTS {
        sqlx::raw_sql(grant)
            .execute(&mut *tx)
            .await
            .map_err(DbError::from_query)?;
    }
    if !superuser {
        // A migrator that isn't a superuser (Neon's owner) hands objects to these roles and
        // replaces theirs: it needs SET and INHERIT on each (D-234).
        crate::nonsuperuser::ensure_memberships(&mut tx).await?;
        // The backstop (D-234): no colophon role keeps CREATE on public, and the deleter
        // membership stays as migration 0048 defines it.
        crate::nonsuperuser::sweep(&mut tx).await?;
    }
    tx.commit().await.map_err(DbError::from_query)?;
    Ok(outcomes)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Neon's refusal, exactly as its control plane worded it on 2026-11-21, and nothing else.
    #[test]
    fn only_neon_s_refusal_of_a_verifier_counts() {
        let neon = r#"Received HTTP code 400 from control plane: {"request_id":"596824af-5584-4af7-91d9-b9c5cca60faf","error":"Neon only supports being given plaintext passwords"}"#;
        assert!(refuses_verifier(neon));
        for other in [
            "permission denied to create role",
            r#"role "colophon_app" already exists"#,
            "password is too short",
            "Received HTTP code 500 from control plane: {\"error\":\"internal error\"}",
            "",
        ] {
            assert!(!refuses_verifier(other), "{other}");
        }
    }

    /// The text form quotes for `standard_conforming_strings = on`: quotes doubled, a backslash
    /// left as it is.
    #[test]
    fn a_text_password_is_one_literal() {
        assert_eq!(text_literal("abc-DEF_123"), "'abc-DEF_123'");
        assert_eq!(text_literal("it's"), "'it''s'");
        assert_eq!(
            text_literal("a\\'; DROP ROLE x; --"),
            "'a\\''; DROP ROLE x; --'"
        );
    }

    #[test]
    fn development_and_short_passwords_are_refused() {
        assert_eq!(
            check_password("colophon-dev"),
            Err(PasswordRefused::Development)
        );
        assert_eq!(
            check_password("fifteen-chars!!"),
            Err(PasswordRefused::TooShort)
        );
        assert_eq!(check_password("sixteen-chars!!!"), Ok(()));
        let message = InitError::Password {
            role: "colophon_app",
            reason: PasswordRefused::Development,
        }
        .to_string();
        assert!(!message.contains("colophon-dev"), "{message}");
    }

    /// RFC 7677's worked exchange (user `user`, password `pencil`): the verifier's `StoredKey` and
    /// `ServerKey` must reproduce the client's proof and the server's signature it shows.
    #[test]
    fn the_verifier_matches_the_rfc_7677_exchange() {
        let b64 = base64::engine::general_purpose::STANDARD;
        let salt = b64
            .decode("W22ZaJ0SNY7soEsUEjb6gQ==")
            .expect("the RFC's salt");
        let verifier = scram_verifier("pencil", &salt);
        let (head, keys) = verifier.rsplit_once('$').expect("three parts");
        assert_eq!(head, "SCRAM-SHA-256$4096:W22ZaJ0SNY7soEsUEjb6gQ==");
        let (stored, server) = keys.split_once(':').expect("two keys");
        let stored = b64.decode(stored).expect("base64");
        let server = b64.decode(server).expect("base64");

        let auth_message = "n=user,r=rOprNGfwEbeRWgbNEkqO,\
            r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0,s=W22ZaJ0SNY7soEsUEjb6gQ==,i=4096,\
            c=biws,r=rOprNGfwEbeRWgbNEkqO%hvYDpWUa2RaTCAfuxFIlj)hNlF$k0";
        let server_signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, &server),
            auth_message.as_bytes(),
        );
        assert_eq!(
            b64.encode(server_signature.as_ref()),
            "6rriTRBi23WpRR/wtup+mMhUZUn/dB5nLTJRsjl95G4="
        );
        let client_signature = hmac::sign(
            &hmac::Key::new(hmac::HMAC_SHA256, &stored),
            auth_message.as_bytes(),
        );
        let proof = b64
            .decode("dHzbZapWIk4jUhN+Ute9ytag9zjfMHgsqmmiz7AndVQ=")
            .expect("the RFC's proof");
        let client_key: Vec<u8> = proof
            .iter()
            .zip(client_signature.as_ref())
            .map(|(a, b)| a ^ b)
            .collect();
        assert_eq!(
            digest::digest(&digest::SHA256, &client_key).as_ref(),
            stored.as_slice()
        );
    }

    #[test]
    fn a_verifier_cannot_end_its_literal() {
        let verifier = scram_verifier("a password with ' quotes \" and $", &[0xff; 16]);
        assert!(
            verifier
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || "+/=$:-".contains(c)),
            "{verifier}"
        );
    }

    /// `deploy/postgres/roles.sql` (the local stack's) is these lists with the public password:
    /// a role or grant added to one and not the other fails here.
    #[test]
    fn the_development_script_matches_these_lists() {
        let script = include_str!("../../../deploy/postgres/roles.sql");
        let created: Vec<String> = script
            .lines()
            .map(str::trim)
            .filter_map(|line| line.strip_prefix("CREATE ROLE "))
            .map(|rest| {
                rest.trim_end_matches(';')
                    .replace(" PASSWORD 'colophon-dev'", "")
            })
            .collect();
        let expected: Vec<String> = ROLES
            .iter()
            .map(|(name, attributes)| format!("{name} {attributes}"))
            .collect();
        assert_eq!(created, expected);
        let granted: Vec<&str> = script
            .lines()
            .map(str::trim)
            .filter(|line| line.starts_with("GRANT "))
            .map(|line| line.trim_end_matches(';'))
            .collect();
        assert_eq!(granted, GRANTS);
    }
}
