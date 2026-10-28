//! For build scripts: works out the commit and the build date and hands them to the compiler as
//! `COLOPHON_COMMIT` and `COLOPHON_BUILD_DATE`, which [`crate::build_info!`] and
//! [`crate::long_version!`] read back.
//!
//! The commit is the first of these that answers:
//!
//! 1. `COLOPHON_COMMIT` in the environment: a packager building from a source tarball sets it;
//! 2. `.cargo_vcs_info.json` beside the manifest: `cargo package` records the commit there, so a
//!    crate built from a registry knows it;
//! 3. `git rev-parse HEAD`, but only when that repository tracks this package's `Cargo.toml` (a
//!    source tree unpacked inside some other repository must not take that repository's commit);
//! 4. otherwise [`crate::UNKNOWN`].
//!
//! The date is `SOURCE_DATE_EPOCH` when it is set (reproducible builds: the release workflow sets
//! it to the tagged commit's time), otherwise the day the script ran, UTC. The script reruns when
//! the commit changes or either variable does, so in a development build the date is when the
//! stamp was last made, which can be earlier than the latest compile.

use std::{
    path::{Path, PathBuf},
    process::Command,
    time::{SystemTime, UNIX_EPOCH},
};

use crate::UNKNOWN;

/// Prints the `cargo::` directives that stamp the calling package. Call it from `build.rs`.
pub fn emit() {
    println!("cargo::rerun-if-changed=build.rs");
    println!("cargo::rerun-if-env-changed=COLOPHON_COMMIT");
    println!("cargo::rerun-if-env-changed=SOURCE_DATE_EPOCH");
    let stamp = std::env::var_os("CARGO_MANIFEST_DIR")
        .map(PathBuf::from)
        .ok_or_else(|| {
            "CARGO_MANIFEST_DIR is not set: emit() runs only in a build script".to_owned()
        })
        .and_then(|dir| Ok((commit(&dir)?, build_date()?)));
    match stamp {
        Ok((commit, date)) => {
            println!("cargo::rustc-env=COLOPHON_COMMIT={commit}");
            println!("cargo::rustc-env=COLOPHON_BUILD_DATE={date}");
        }
        // Fails the build once the script exits (Cargo 1.84+): a wrong stamp is worse than none.
        Err(message) => println!("cargo::error={message}"),
    }
}

fn commit(manifest_dir: &Path) -> Result<String, String> {
    if let Some(given) = std::env::var("COLOPHON_COMMIT")
        .ok()
        .filter(|value| !value.is_empty())
    {
        let given = given.to_ascii_lowercase();
        return if is_commit_id(&given) {
            Ok(given)
        } else {
            Err(format!(
                "COLOPHON_COMMIT={given:?} is not a full git commit id (40 or 64 hex digits)"
            ))
        };
    }
    let vcs_info = manifest_dir.join(".cargo_vcs_info.json");
    if let Ok(text) = std::fs::read_to_string(&vcs_info) {
        println!("cargo::rerun-if-changed={}", vcs_info.display());
        if let Some(commit) = commit_from_vcs_info(&text) {
            return Ok(commit);
        }
    }
    Ok(from_git(manifest_dir).unwrap_or_else(|| UNKNOWN.to_owned()))
}

/// The commit from git, if git is installed and its repository tracks this package.
fn from_git(dir: &Path) -> Option<String> {
    git(dir, &["ls-files", "--error-unmatch", "Cargo.toml"])?;
    let head = git(dir, &["rev-parse", "--verify", "HEAD"])?;
    if !is_commit_id(&head) {
        return None;
    }
    watch_git(dir);
    Some(head)
}

/// Asks Cargo to rerun the script when HEAD moves. Only existing paths are named, since Cargo
/// reruns every time for a path that doesn't exist.
///
/// - `HEAD` in the worktree's git directory: a checkout of another branch;
/// - `logs/HEAD`, the worktree's reflog: every commit, merge, reset or checkout appends to it;
/// - the branch's loose ref and `packed-refs` in the common directory. A packed branch gets a
///   loose ref file on its next commit, so without a reflog the nearest existing directory above
///   that file is watched instead (it then also reruns for other branches' commits).
fn watch_git(dir: &Path) {
    let absolute = |path: String| {
        let path = PathBuf::from(path);
        if path.is_absolute() {
            path
        } else {
            dir.join(path)
        }
    };
    let mut paths = Vec::new();
    let mut reflog = false;
    if let Some(git_dir) = git(dir, &["rev-parse", "--git-dir"]).map(absolute) {
        paths.push(git_dir.join("HEAD"));
        let log = git_dir.join("logs").join("HEAD");
        reflog = log.exists();
        paths.push(log);
    }
    if let Some(common) = git(dir, &["rev-parse", "--git-common-dir"]).map(absolute) {
        paths.push(common.join("packed-refs"));
        // `refs/heads/<branch>`; nothing when HEAD is detached, which HEAD and its log cover.
        if let Some(reference) = git(dir, &["symbolic-ref", "-q", "HEAD"]) {
            let loose = common.join(reference);
            if !reflog
                && !loose.exists()
                && let Some(parent) = loose.ancestors().skip(1).find(|path| path.exists())
            {
                paths.push(parent.to_path_buf());
            }
            paths.push(loose);
        }
    }
    for path in paths.into_iter().filter(|path| path.exists()) {
        println!("cargo::rerun-if-changed={}", path.display());
    }
}

/// Runs git in `dir`; its trimmed output when it succeeds.
fn git(dir: &Path, args: &[&str]) -> Option<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8(output.stdout)
        .ok()
        .map(|text| text.trim().to_owned())
}

/// A full commit id: 40 hex digits (SHA-1) or 64 (SHA-256 repositories), lower case.
fn is_commit_id(text: &str) -> bool {
    matches!(text.len(), 40 | 64)
        && text
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// The commit `cargo package` recorded: `{"git": {"sha1": "…"}, …}`. Read by hand to keep build
/// scripts free of dependencies; anything unexpected is `None`.
fn commit_from_vcs_info(text: &str) -> Option<String> {
    let key = "\"sha1\"";
    let rest = &text[text.find(key)? + key.len()..];
    let rest = rest
        .trim_start()
        .strip_prefix(':')?
        .trim_start()
        .strip_prefix('"')?;
    let value = &rest[..rest.find('"')?];
    is_commit_id(value).then(|| value.to_owned())
}

fn build_date() -> Result<String, String> {
    let seconds = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) if !value.trim().is_empty() => value
            .trim()
            .parse::<i64>()
            .map_err(|_| format!("SOURCE_DATE_EPOCH={value:?} is not a whole number of seconds"))?,
        _ => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
            .ok_or_else(|| "the system clock is outside the range a date can show".to_owned())?,
    };
    Ok(date(seconds))
}

/// The UTC day of a Unix time, as `YYYY-MM-DD`.
fn date(seconds: i64) -> String {
    let (year, month, day) = civil_from_days(seconds.div_euclid(86_400));
    format!("{year:04}-{month:02}-{day:02}")
}

/// Days since 1970-01-01 to a proleptic Gregorian (year, month, day): Howard Hinnant's
/// `civil_from_days` (<https://howardhinnant.github.io/date_algorithms.html>).
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let day_of_era = z.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unix_times_become_utc_days() {
        // Day numbers from Python's datetime: (date - date(1970, 1, 1)).days.
        for (days, expected) in [
            (0, "1970-01-01"),
            (-1, "1969-12-31"),
            (11_017, "2000-03-01"),
            (19_782, "2024-02-29"),
            (20_754, "2026-10-28"),
            (47_541, "2100-03-01"),
        ] {
            assert_eq!(date(days * 86_400), expected, "day {days}");
        }
        // The last second of a day is still that day; the next is the next.
        assert_eq!(date(1_793_231_999), "2026-10-28");
        assert_eq!(date(1_793_232_000), "2026-10-29");
    }

    #[test]
    fn commit_ids_are_full_lower_case_hex() {
        assert!(is_commit_id(&"a".repeat(40)));
        assert!(is_commit_id(&"0123456789abcdef".repeat(4)));
        assert!(!is_commit_id("50558a7"), "short ids are ambiguous");
        assert!(!is_commit_id(&"A".repeat(40)));
        assert!(!is_commit_id(&"g".repeat(40)));
        assert!(!is_commit_id(""));
    }

    /// Runs git with a throwaway identity and no signing, whatever the machine's config says.
    fn run_git(dir: &Path, args: &[&str]) -> bool {
        Command::new("git")
            .args([
                "-c",
                "user.name=test",
                "-c",
                "user.email=test@example.invalid",
            ])
            .args([
                "-c",
                "commit.gpgsign=false",
                "-c",
                "core.hooksPath=/dev/null",
            ])
            .args(args)
            .current_dir(dir)
            .output()
            .is_ok_and(|output| output.status.success())
    }

    #[test]
    fn git_gives_its_commit_only_to_a_package_it_tracks() {
        let repo = std::env::temp_dir().join(format!("colophon-stamp-{}", std::process::id()));
        let _gone = std::fs::remove_dir_all(&repo);
        let tracked = repo.join("tracked");
        let unpacked = repo.join("unpacked");
        std::fs::create_dir_all(&tracked).expect("dir");
        std::fs::create_dir_all(&unpacked).expect("dir");
        std::fs::write(tracked.join("Cargo.toml"), "[package]\n").expect("write");
        if !run_git(&repo, &["init", "-q"]) {
            eprintln!("git is not installed: skipping (the fallback is `unknown`)");
            return;
        }
        assert!(run_git(&repo, &["add", "tracked/Cargo.toml"]));
        assert!(run_git(&repo, &["commit", "-q", "-m", "one"]));
        // A source tree unpacked inside this repository, but not part of it.
        std::fs::write(unpacked.join("Cargo.toml"), "[package]\n").expect("write");

        let head = git(&repo, &["rev-parse", "HEAD"]).expect("HEAD");
        assert_eq!(from_git(&tracked), Some(head));
        assert_eq!(from_git(&unpacked), None);
        let outside = std::env::temp_dir().join(format!("colophon-nogit-{}", std::process::id()));
        std::fs::create_dir_all(&outside).expect("dir");
        assert_eq!(from_git(&outside), None, "no repository at all");
        let _gone = std::fs::remove_dir_all(&repo);
        let _gone = std::fs::remove_dir_all(&outside);
    }

    #[test]
    fn the_packaged_commit_is_read_from_cargo_vcs_info() {
        let sha = "50558a7c0ffee50558a7c0ffee50558a7c0ffee5";
        let packaged = format!(
            "{{\n  \"git\": {{\n    \"sha1\": \"{sha}\",\n    \"dirty\": true\n  }},\n  \"path_in_vcs\": \"crates/colophon-cli\"\n}}"
        );
        assert_eq!(commit_from_vcs_info(&packaged).as_deref(), Some(sha));
        assert_eq!(
            commit_from_vcs_info(&format!("{{\"git\":{{\"sha1\":\"{sha}\"}}}}")).as_deref(),
            Some(sha)
        );
        for broken in [
            "",
            "{}",
            "{\"git\": {\"sha1\": 7}}",
            "{\"git\": {\"sha1\": \"not-a-commit\"}}",
            "{\"git\": {\"sha1\": \"",
        ] {
            assert_eq!(commit_from_vcs_info(broken), None, "{broken:?}");
        }
    }
}
