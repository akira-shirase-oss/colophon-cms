//! The setup code (D-060, amending D-043). While the installation has no administrator, a server
//! that serves the API issues one code when it starts, stores only its hash, and prints the code
//! to standard error, its own terminal: never to the structured log, which may be shipped
//! elsewhere. A signed-in person who enters it in the Studio becomes the first administrator.
//! Once anyone is an administrator, nothing is issued or printed.

use std::time::Duration;

use colophon_auth::tokens;
use colophon_db::{PgPool, setup};
use time::{OffsetDateTime, UtcOffset};
use tokio_util::sync::CancellationToken;

/// How long a code works.
pub const LIFETIME: Duration = Duration::from_hours(1);
/// How many times issuing is tried while the database can't be reached, with growing pauses
/// (about five minutes in all), before the server gives up and says so.
const ATTEMPTS: u32 = 12;
/// The longest a write to standard error may take.
const WRITE_DEADLINE: Duration = Duration::from_secs(5);

/// What the operator reads: the code, where to enter it, until when, and the alternative.
pub fn banner(code: &str, studio: &str, expires: OffsetDateTime) -> String {
    let expires = expires.to_offset(UtcOffset::UTC);
    let rule = "=".repeat(78);
    format!(
        "\n{rule}\nColophon has no administrator yet.\n\n    Setup code: {code}\n\n\
         To become its first administrator, open the Studio, sign in or create an\n\
         account, and enter this code:\n    {studio}\n\
         It works once, until {:02}:{:02} UTC. Restart the server for a new one.\n\n\
         Or, with the database credentials, run:\n    \
         colophon admin bootstrap --subject <your subject>\n{rule}\n\n",
        expires.hour(),
        expires.minute(),
    )
}

/// Issues and prints a code if the installation needs one; returns when done, when it gave up,
/// or when `cancel` fires.
pub async fn run(pool: PgPool, studio: String, cancel: CancellationToken) {
    let Ok(code) = tokens::setup_code() else {
        tracing::error!("no secure random source: no setup code was issued");
        return;
    };
    let Some(hash) = tokens::setup_code_hash(&code) else {
        tracing::error!("a generated setup code didn't read back: no setup code was issued");
        return;
    };
    let mut pause = Duration::from_secs(1);
    for attempt in 1..=ATTEMPTS {
        let issued = tokio::select! {
            () = cancel.cancelled() => return,
            issued = setup::issue(&pool, &hash, LIFETIME) => issued,
        };
        match issued {
            Ok(Some(expires)) => {
                let text = banner(&code, &studio, expires);
                // A terminal write can block (a full pipe): off the runtime, with a deadline.
                let write = tokio::task::spawn_blocking(move || {
                    use std::io::Write as _;
                    let mut stderr = std::io::stderr().lock();
                    stderr.write_all(text.as_bytes())?;
                    stderr.flush()
                });
                let written = tokio::time::timeout(WRITE_DEADLINE, write).await;
                if !matches!(written, Ok(Ok(Ok(())))) {
                    tracing::warn!(
                        "a setup code was issued but couldn't be printed; restart the server for another"
                    );
                }
                // The log says where to look, never what the code is.
                tracing::info!(
                    expires = %expires,
                    "this installation has no administrator: a setup code was printed to standard error"
                );
                return;
            }
            Ok(None) => {
                // Either someone is an administrator (the usual case: say nothing), or ten codes
                // are still live, which the operator should hear about.
                if matches!(setup::has_administrator(&pool).await, Ok(false)) {
                    tracing::warn!(
                        "this installation has no administrator, and ten setup codes are still live, so this start printed none: use one printed earlier, wait for them to expire, or run `colophon admin bootstrap`"
                    );
                }
                return;
            }
            Err(error) => {
                tracing::warn!(%error, attempt, "couldn't check whether this installation needs a setup code; trying again");
                tokio::select! {
                    () = cancel.cancelled() => return,
                    () = tokio::time::sleep(pause) => {}
                }
                pause = (pause * 2).min(Duration::from_secs(30));
            }
        }
    }
    tracing::warn!(
        "gave up issuing a setup code: the database didn't answer. Restart the server once it does, or run `colophon admin bootstrap`"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_banner_names_the_code_the_studio_and_the_expiry_in_utc() {
        let expires = OffsetDateTime::from_unix_timestamp(1_800_000_000)
            .expect("a valid time")
            .to_offset(UtcOffset::from_hms(2, 0, 0).expect("an offset"));
        let text = banner(
            "7K3QD-0ZX1V-ABCDE-FGHJK",
            "http://localhost:58400/studio/",
            expires,
        );
        assert!(
            text.contains("Setup code: 7K3QD-0ZX1V-ABCDE-FGHJK"),
            "{text}"
        );
        assert!(
            text.contains("\n    http://localhost:58400/studio/\n"),
            "{text}"
        );
        // 1,800,000,000 is 2027-01-15 08:00 UTC, whatever offset it was given in.
        assert!(text.contains("until 08:00 UTC"), "{text}");
        assert!(text.contains("colophon admin bootstrap"), "{text}");
        assert_eq!(text.matches("7K3QD").count(), 1, "the code appears once");
        assert!(
            text.lines().all(|line| line.chars().count() <= 78),
            "{text}"
        );
    }
}
