<script>
	import Status from '#lib/components/Status.svelte';
</script>

**Being built. Nothing on this page works yet.** Scheduled publishing is being built on the branch `e/schedules`, which isn't merged: it doesn't exist on `main` or in any release. This page describes the behaviour the master specification asks for; the last section says what the branch has so far. Everything here can still change.

Scheduled publishing lets an approved publish request go live at a time someone chooses, in their own time zone. The hard parts aren't the timer: they are a local time that happens twice or never when the clocks change, content edited after it was approved, a person whose role changed before the time came, and a server that was down at the moment it was due.

## What the spec asks

The master spec's "Scheduled publishing" contract (§06) and its "Rollback, unpublish and scheduling" section (§14) ask for this:

- **Store what was chosen and what it means**: the local time, the IANA time zone (`Europe/London`), the resolved UTC instant and the approval hash.
- **No guessing at clock changes.** A local time that is ambiguous (the clocks go back and show it twice) or doesn't exist (they go forward and skip it) needs an explicit choice.
- **Check everything again when it falls due**: authority, the approval, the revision hashes and dependencies. Content edited after approval **blocks** the schedule; it never silently changes what the scheduled release publishes.
- **Late is labelled late.** A schedule that runs after its time, because the platform was down, is shown as *delayed*, never as on time.
- **Cancelling stops new work, not finished work.** Cancellation prevents the publish if it hasn't happened; it doesn't undo a release already committed.

Case **X07** in the spec's verification suite tests it: a scheduled publication that crosses a daylight-saving change, or whose approval expires, must publish at an explicitly chosen instant, be blocked by revalidation when the approval is stale, and never have a duplicate effect. The spec asks scheduling tests to advance a fake clock rather than wait or change a system clock.

## How it builds on what exists

A schedule doesn't publish anything new: it publishes one approved [publish request](/concepts/publish-requests/) at the hash it was approved on, as a [release](/concepts/releases-and-generations/), and the release is announced through [the outbox](/concepts/outbox-and-worker/) like any other. A publish request can hold takedowns as well as publishes, so a scheduled one can take entries down at its time.

The checks it needs at execution are the ones a publish already makes: the changeset still has the approved hash, no pinned entry moved on, the approval hasn't expired, and the role may publish in every collection. The rule that generations only move forward still holds: a scheduled release is the next generation, whenever it runs.

## Where it sits in the roadmap

The roadmap on the R.2 branch names scheduling twice. Milestone **P4** lists "scheduled publishing with time zones and daylight-saving handling" and puts X07 in its proof gate. Milestone **E** (editorial parity) lists scheduled publishing and unpublishing, with time zones and missed schedules shown as delayed, and its proof gate says a schedule missed during downtime is reported as delayed, never on time. `COMPARISON.md` places it in E, and the branch is named for E; the branch's own roadmap notes it was built first, on the maintainer's request, ahead of the rest of milestone E. See [Roadmap and status](/resources/roadmap/).

## What the branch has so far (2026-10-27) <Status building="e/schedules" />

Nine commits on `e/schedules`, made on 27 October 2026 on top of R.2's branch and not pushed or merged. Its design decision is recorded as **D-060 on `e/schedules`**; a different decision has the same number on `p3/3.7-onboarding`, so the number will change on one of them.

- **The publish moved into one routine.** `colophon_db::publish::publish` is the API's publish, unchanged in order and locks, so a schedule runs exactly the checks a person's publish makes. A person publishes on the release they saw; a schedule publishes on whatever is current, retrying up to three times if another release commits first.
- **Resolving a time.** `colophon_domain::schedule` resolves a local time in its IANA zone with the `jiff` crate's bundled time-zone database (no I/O, so every machine agrees). A time the clocks show twice or skip is refused with both candidate instants until the request chooses `earlier` or `later`: in New York on 1 November 2026, 01:30 is 05:30Z or 06:30Z. A time must be in the future, at most a year and a day ahead, and before the approval expires.
- **Storing it.** Migration 0019 adds `schedules`: the local time, zone and choice as given, the instant, the changeset hash and the approval it publishes under, and its outcome. One pending schedule per publish request, enforced by a partial unique index.
- **The API.** Scheduling an approved changeset (`POST` to its `schedule` route, with `If-Match` naming the hash the caller saw) needs `content.publish` in every collection it touches. Any member can list schedules; listing returns the pending ones in time order (at most 100) and the 20 most recently finished. The author, or anyone who may publish the request, can cancel a pending one; cancelling something already published is refused, because it never undoes a release.
- **Running it.** A loop in the worker, beside the deliveries and on the worker's own pool, looks for what is due every `COLOPHON_WORKER_POLL_MS`. It opens the project **as the person who scheduled it**, so their role as it is then decides, takes the schedule's row `FOR UPDATE SKIP LOCKED`, publishes, and marks the schedule published **in the same transaction as the release**, so a crash leaves both or neither. Anything that refuses it (an edit after approval, an expired approval, a role that may no longer publish, an author who left the project, a URL name another entry still publishes) blocks it with the reason in plain words, and nothing is published. A start more than 60 seconds late is labelled `delayed`.
- **The Studio.** An approved request's page has a Schedule panel: a date, a time and a time zone (yours first). At a clock change it says the clocks go back or forward that night and shows both moments with their abbreviations. Releases lists what is scheduled, then recent outcomes: on time, delayed, not published and why, or cancelled.
- **Tests.** The domain's tests resolve New York's and London's clock changes; `crates/colophon-db/tests/schedules.rs` passes the clock in instead of waiting, and checks that a schedule publishes once and not before its time, is delayed when late, and is blocked by an expired approval, an edit, a demotion or a departure; two workers at once publish it once. The server, Studio and testbed tests schedule a publish a few seconds ahead and watch the worker publish it as release 1, and walk New York's doubled 01:30.
- **Load.** `perf/k6/schedules.js` lists, schedules and cancels. One smoke run on the maintainer's laptop while it was busy (27 October 2026): 1,637 requests a second, p99 53.7 ms, no failures. That is a smoke run on one laptop, not a full run and not production capacity.

Nothing above is in `main`. Until it is merged, with its review, the page's label stays *Building*.
