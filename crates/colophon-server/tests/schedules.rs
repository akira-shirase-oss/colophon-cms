//! Scheduled publishing end to end (D-060): the real binary, with its worker. An owner schedules
//! an approved publish request a few seconds ahead, in a time zone, and the worker publishes it
//! once, on time; a local time the clocks show twice is refused with both candidate instants until
//! one is chosen; a pending schedule can be cancelled, once; an edited request, a role that can't
//! publish, and a time in the past are refused. Needs the stack and migrations.
#![cfg(unix)]
#![expect(
    clippy::expect_used,
    reason = "process-level tests: any failure to start, read or connect is a test failure"
)]

mod common;

use std::time::{Duration, Instant};

use common::{Reply, Server, Session, call, create_article, join, key, project, sign_in};
use reqwest::{Method, StatusCode};
use serde_json::{Value, json};

async fn send(
    session: &Session,
    method: Method,
    path: &str,
    body: Option<&Value>,
    extra: &[(&str, &str)],
) -> Reply {
    call(session, method, path, body, extra)
        .await
        .expect("request")
}

/// A local time the clocks show twice within `days` days of `now` (seconds), across zones that
/// fall back sometime in the year: (zone, `YYYY-MM-DDTHH:MM`, both instants as RFC 3339). `None`
/// when no backward clock change fits the window.
fn next_ambiguous(now: i64, days: i64) -> Option<(String, String, String, String)> {
    use jiff::{Timestamp, tz::TimeZone};
    const ZONES: &[&str] = &[
        "America/New_York",
        "America/Santiago",
        "Australia/Lord_Howe",
        "Pacific/Auckland",
        "Pacific/Fiji",
        "Africa/Casablanca",
        "Europe/London",
        "America/Nuuk",
        "America/Havana",
        "Asia/Gaza",
        "Australia/Sydney",
        "America/Asuncion",
    ];
    let horizon = now + days * 24 * 60 * 60;
    for zone in ZONES {
        let tz = TimeZone::get(zone).expect("a known zone");
        let start = Timestamp::from_second(now).expect("in range");
        for transition in tz.following(start) {
            let at = transition.timestamp().as_second();
            if at > horizon {
                break;
            }
            let after = transition.offset().seconds();
            let before = tz
                .to_offset(Timestamp::from_second(at - 1).expect("in range"))
                .seconds();
            if after >= before {
                continue;
            }
            // The clocks go back: local times [at + after, at + before) happen twice. A whole
            // minute inside is the local time asked for; the two instants it can mean are it
            // minus each offset.
            let gap = i64::from(before - after);
            let mid = at + i64::from(after) + gap / 2;
            let local_at = mid - mid % 60;
            let local = Timestamp::from_second(local_at)
                .expect("in range")
                .to_zoned(TimeZone::UTC)
                .datetime()
                .strftime("%Y-%m-%dT%H:%M")
                .to_string();
            let instant = |at| Timestamp::from_second(at).expect("in range").to_string();
            return Some((
                (*zone).to_owned(),
                local,
                instant(local_at - i64::from(before)),
                instant(local_at - i64::from(after)),
            ));
        }
    }
    None
}

/// `next_ambiguous` over a year finds a real repeated local time, and its two instants are the
/// ones disambiguation would give.
#[test]
fn next_ambiguous_finds_a_real_fold() {
    use jiff::tz::{AmbiguousOffset, Disambiguation, TimeZone};
    let now = jiff::Timestamp::now().as_second();
    let (zone, local, earlier, later) = next_ambiguous(now, 364).expect("some zone falls back");
    let civil: jiff::civil::DateTime = local.parse().expect("a civil time");
    let tz = TimeZone::get(&zone).expect("a known zone");
    let ambiguous = tz.to_ambiguous_zoned(civil);
    assert!(
        matches!(ambiguous.offset(), AmbiguousOffset::Fold { .. }),
        "{local} in {zone} is not a repeated local time"
    );
    let at = |how| {
        ambiguous
            .clone()
            .disambiguate(how)
            .expect("a fold disambiguates")
            .timestamp()
            .to_string()
    };
    assert_eq!(at(Disambiguation::Earlier), earlier);
    assert_eq!(at(Disambiguation::Later), later);
}

/// An approved publish request for a new article: (its path, its If-Match).
async fn approved(owner: &Session, project: &str, title: &str) -> (String, String) {
    let base = format!("/api/v1/projects/{project}/environments/production");
    let entry = create_article(owner, project, title).await;
    let set = send(
        owner,
        Method::POST,
        &format!("{base}/changesets"),
        Some(&json!({"title": title, "entries": [entry]})),
        &[("idempotency-key", &key())],
    )
    .await;
    assert_eq!(set.status, StatusCode::CREATED, "{}", set.body);
    let path = format!(
        "{base}/changesets/{}",
        set.body["data"]["id"].as_str().expect("an id")
    );
    let if_match = format!("\"{}\"", set.body["data"]["hash"].as_str().expect("a hash"));
    for (step, body) in [
        ("submit", None),
        ("reviews", Some(json!({"decision": "approve"}))),
    ] {
        let done = send(
            owner,
            Method::POST,
            &format!("{path}/{step}"),
            body.as_ref(),
            &[("if-match", &if_match)],
        )
        .await;
        assert!(done.status.is_success(), "{step}: {}", done.body);
    }
    (path, if_match)
}

/// `seconds` from now, as a local time in UTC.
fn utc_in(seconds: i64) -> String {
    (jiff::Timestamp::now() + jiff::SignedDuration::from_secs(seconds))
        .strftime("%Y-%m-%dT%H:%M:%S")
        .to_string()
}

#[tokio::test]
#[ignore = "needs the local stack and migrations: scripts/stack.sh up && scripts/migrate.sh"]
#[expect(
    clippy::too_many_lines,
    reason = "one story, told in order: schedule, publish, choose, cancel, refuse"
)]
async fn a_scheduled_publish_request_goes_live_once_at_its_time() {
    let _server = Server::start().await;
    let owner = sign_in("dev-owner").await;
    let project = project(&owner).await;
    let base = format!("/api/v1/projects/{project}/environments/production");
    let (path, if_match) = approved(&owner, &project, "Spring menu").await;

    // Three seconds ahead, in UTC: the worker publishes it, once.
    let made = send(
        &owner,
        Method::POST,
        &format!("{path}/schedule"),
        Some(&json!({"local_time": utc_in(3), "time_zone": "UTC"})),
        &[("if-match", &if_match)],
    )
    .await;
    assert_eq!(made.status, StatusCode::CREATED, "{}", made.body);
    assert_eq!(made.body["data"]["state"], "scheduled");
    let again = send(
        &owner,
        Method::POST,
        &format!("{path}/schedule"),
        Some(&json!({"local_time": utc_in(60), "time_zone": "UTC"})),
        &[("if-match", &if_match)],
    )
    .await;
    assert_eq!(again.status, StatusCode::CONFLICT, "one pending schedule");
    let deadline = Instant::now() + Duration::from_secs(20);
    let ran = loop {
        let listed = send(&owner, Method::GET, &format!("{base}/schedules"), None, &[]).await;
        let first = listed.body["data"][0].clone();
        if first["state"] != "scheduled" {
            break first;
        }
        assert!(Instant::now() < deadline, "the schedule never ran: {first}");
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    assert_eq!(ran["state"], "published", "{ran}");
    assert_eq!(ran["generation"], 1);
    assert_eq!(ran["delayed"], false);
    let live = send(&owner, Method::GET, &format!("{base}/releases"), None, &[]).await;
    assert_eq!(
        live.headers
            .get("colophon-generation")
            .and_then(|value| value.to_str().ok()),
        Some("1")
    );

    // A local time the clocks show twice needs a choice: refused with both candidates, then the
    // later taken. The window is the approval's week and no clock change may fall inside it; when
    // none does that part is skipped, and a plain pending schedule takes the cancel.
    let (path, if_match) = approved(&owner, &project, "Opening hours").await;
    let pending = if let Some((zone, local, earlier, later)) =
        next_ambiguous(jiff::Timestamp::now().as_second(), 6)
    {
        let refused = send(
            &owner,
            Method::POST,
            &format!("{path}/schedule"),
            Some(&json!({"local_time": local, "time_zone": zone})),
            &[("if-match", &if_match)],
        )
        .await;
        assert_eq!(
            refused.status,
            StatusCode::UNPROCESSABLE_ENTITY,
            "{}",
            refused.body
        );
        let details = &refused.body["error"]["details"];
        assert_eq!(details["reason"], "ambiguous_local_time");
        assert_eq!(details["candidates"], json!([earlier, later]));
        let chosen = send(
            &owner,
            Method::POST,
            &format!("{path}/schedule"),
            Some(&json!({"local_time": local, "time_zone": zone, "choice": "later"})),
            &[("if-match", &if_match)],
        )
        .await;
        assert_eq!(chosen.status, StatusCode::CREATED, "{}", chosen.body);
        assert_eq!(chosen.body["data"]["run_at"], later);
        assert_eq!(chosen.body["data"]["local_time"], format!("{local}:00"));
        assert_eq!(chosen.body["data"]["choice"], "later");
        chosen.body["data"]["id"].clone()
    } else {
        eprintln!("no backward clock change within a week: the ambiguous-time part is skipped");
        let pending = send(
            &owner,
            Method::POST,
            &format!("{path}/schedule"),
            Some(&json!({"local_time": utc_in(5 * 24 * 60 * 60), "time_zone": "UTC"})),
            &[("if-match", &if_match)],
        )
        .await;
        assert_eq!(pending.status, StatusCode::CREATED, "{}", pending.body);
        pending.body["data"]["id"].clone()
    };

    // Cancelled once; a second cancel says so.
    let schedule = format!("{base}/schedules/{}", pending.as_str().expect("an id"));
    let cancelled = send(&owner, Method::DELETE, &schedule, None, &[]).await;
    assert_eq!(cancelled.status, StatusCode::OK, "{}", cancelled.body);
    assert_eq!(cancelled.body["data"]["state"], "cancelled");
    let twice = send(&owner, Method::DELETE, &schedule, None, &[]).await;
    assert_eq!(twice.status, StatusCode::CONFLICT);

    // In the past, at a stale hash, or by a role that can't publish: refused.
    let past = send(
        &owner,
        Method::POST,
        &format!("{path}/schedule"),
        Some(&json!({"local_time": utc_in(-60), "time_zone": "UTC"})),
        &[("if-match", &if_match)],
    )
    .await;
    assert_eq!(past.status, StatusCode::UNPROCESSABLE_ENTITY);
    assert_eq!(past.body["error"]["details"]["reason"], "in_the_past");
    let stale = send(
        &owner,
        Method::POST,
        &format!("{path}/schedule"),
        Some(&json!({"local_time": utc_in(60), "time_zone": "UTC"})),
        &[(
            "if-match",
            "\"sha256:0000000000000000000000000000000000000000000000000000000000000000\"",
        )],
    )
    .await;
    assert_eq!(stale.status, StatusCode::CONFLICT);
    let editor = sign_in("dev-editor").await;
    join(&owner, &editor, &project, "editor").await;
    let forbidden = send(
        &editor,
        Method::POST,
        &format!("{path}/schedule"),
        Some(&json!({"local_time": utc_in(60), "time_zone": "UTC"})),
        &[("if-match", &if_match)],
    )
    .await;
    assert_eq!(
        forbidden.status,
        StatusCode::FORBIDDEN,
        "{}",
        forbidden.body
    );
    let unknown = send(
        &owner,
        Method::POST,
        &format!("{path}/schedule"),
        Some(&json!({"local_time": utc_in(60), "time_zone": "Mars/Olympus_Mons"})),
        &[("if-match", &if_match)],
    )
    .await;
    assert_eq!(
        unknown.body["error"]["details"]["reason"],
        "unknown_time_zone"
    );
}
