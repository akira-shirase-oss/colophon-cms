<script>
	import Status from '#lib/components/Status.svelte';
</script>

Scheduled publishing lets an approved publish request go live at a time someone chooses, in their own time zone. It was built ahead of the rest of milestone E (editorial parity), on the maintainer's request, under decision D-060. The hard parts aren't the timer: they are a local time that happens twice or never when the clocks change, content edited after it was approved, a person whose role changed before the time came, and a server that was down at the moment it was due.

## What the spec asks

The master spec's "Scheduled publishing" contract (§06) and its "Rollback, unpublish and scheduling" section (§14) ask for this:

- **Store what was chosen and what it means**: the local time, the IANA time zone (`Europe/London`), the resolved UTC instant and the approval hash.
- **No guessing at clock changes.** A local time that is ambiguous (the clocks go back and show it twice) or doesn't exist (they go forward and skip it) needs an explicit choice.
- **Check everything again when it falls due**: authority, the approval, the revision hashes and dependencies. Content edited after approval **blocks** the schedule; it never silently changes what the scheduled release publishes.
- **Late is labelled late.** A schedule that runs after its time, because the platform was down, is shown as *delayed*, never as on time.
- **Cancelling stops new work, not finished work.** Cancellation prevents the publish if it hasn't happened; it doesn't undo a release already committed.

Case **X07** in the spec's verification suite tests it: a scheduled publication that crosses a daylight-saving change, or whose approval expires, must publish at an explicitly chosen instant, be blocked by revalidation when the approval is stale, and never have a duplicate effect. The spec asks scheduling tests to advance a fake clock rather than wait or change a system clock.

## How it builds on what exists

A schedule doesn't publish anything new: it publishes one approved [publish request](/concepts/publish-requests/) at the hash it was approved on, as a [release](/concepts/releases-and-generations/), and the release is announced through [the outbox](/concepts/outbox-and-worker/) like any other. The rule that generations only move forward still holds: a scheduled release is the next generation, whenever it runs.

The publish itself moved into one routine, `colophon_db::publish::publish`, which the API's publish and the scheduler both run, unchanged in order and locks. So a schedule makes exactly the checks a person's publish makes: the publish request still has the approved hash, no entry in it moved on, the approval hasn't expired, and the role may publish in every collection. A person publishes on the release they saw; a schedule publishes on whatever is current, retrying up to three times if another release commits first.

## Choosing a time

`colophon_domain::schedule` resolves a local time in its IANA zone with the `jiff` crate's bundled time-zone database: no I/O, so every machine agrees. A time the clocks show twice or skip is refused with both candidate instants until the request chooses `earlier` or `later`: in New York on 1 November 2026, 01:30 is 05:30Z or 06:30Z. A time must be in the future, at most a year and a day ahead, and before the approval it publishes under expires.

Scheduling (`POST` to a publish request's `schedule` route, with `If-Match` naming the hash the caller saw) needs `content.publish` in every collection the request touches, exactly as publishing would, now. A publish request has at most one pending schedule. The [publish requests reference](/reference/api/publish-requests/) has the routes and their errors.

## Storing it

Migration 0019 adds `schedules`: the local time, zone and choice as given, the instant, the publish request's hash and the approval it publishes under, who scheduled it, and its outcome (`scheduled`, `published`, `blocked` or `cancelled`, with a reason in plain words when blocked). A partial unique index allows one pending schedule per publish request. Any member can list schedules: the pending ones in time order (at most 100) and the 20 most recently finished, each through its own index, so a list stays small however many have run.

## Running it

A loop in the worker, beside the deliveries and on the worker's own pool, looks for what is due every `COLOPHON_WORKER_POLL_MS`, taking at most 20 at a time in time order. For each, it opens the project **as the person who scheduled it**, so their role as it is then decides, takes the schedule's row `FOR UPDATE SKIP LOCKED`, publishes, and marks the schedule published **in the same transaction as the release**, so a crash leaves both or neither and two workers never both publish it.

Anything that refuses the publish blocks the schedule with the reason, and nothing is published: an edit after approval, an expired approval, a role that may no longer publish, an author who left the project, or a URL name another entry still publishes. A start more than 60 seconds after its time is labelled `delayed`.

The author, or anyone who may publish the request, can cancel a pending schedule. Cancelling waits for a run in progress, and cancelling something already published is refused, because it never undoes a release.

## In the Studio

An approved publish request's page has a Schedule panel: a date, a time and a time zone, yours first. At a clock change it says the clocks go back or forward that night and shows both moments with their zone abbreviations, and schedules the one you pick. A pending schedule shows when it publishes, there and where you are, who set it, and a button to cancel it; a past one says it published on time, published late, didn't publish and why, or was cancelled. The Releases page lists what is scheduled, then recent outcomes, and shows the new release when one publishes.

## How it is tested

- `crates/colophon-domain/src/schedule.rs`: ordinary times, times the clocks show twice and times they skip (New York and London), nonsense refused by name, and the window: in the future, within a year and a day, before the approval expires.
- `crates/colophon-db/tests/schedules.rs`, with the clock passed in instead of waited for: a schedule publishes once and not before its time, is labelled delayed when late, and is blocked by an expired approval, an edit, a demotion or a departure; two workers at once publish it once.
- `crates/colophon-server/tests/schedules.rs`, `apps/studio/tests/server/schedules.spec.ts` and `apps/testbed/tests/schedules.spec.ts`: schedule a publish a few seconds ahead and watch the worker publish it as the next release, and walk New York's doubled 01:30.

## Load

`perf/k6/schedules.js` lists, schedules and cancels. `scripts/check.sh` and CI run it as a smoke, CI at 500 requests a second, about half what its slowest runners manage. Its first smoke missed its 200 ms p99 because a list sorted every schedule ever made; lists now read pending and recently finished schedules through their own indexes (a list's p99 went from 586 ms to 46 ms on a busy laptop).

On the maintainer's laptop (Apple M2 Pro, 12 cores, 32 GB), on commit `db542c4` with a fresh database, on 27 October 2026: 1,000,262 requests at 1,856 a second with a p99 of 48.4 ms, and a steady run at 1,000 a second with a p99 of 10.5 ms, nothing failed in either ([performance](/resources/performance/)). That is one laptop, not production capacity.

## Not built yet

- **Scheduled takedowns, tested.** A publish request can hold takedowns as well as publishes, and a schedule runs the same publish, so a scheduled request takes its entries down at its time; no test covers that case yet. <Status planned="E" />
- **The rest of milestone E**: localization, review stages, comments and tasks, live collaboration, live queries and visual preview. <Status planned="E" />
- Recurring schedules are not planned (D-060).

## Design

Decision D-060 in the [decisions](/design/decisions/); [module 02, db](/design/modules/02-db/) and [module 05, worker](/design/modules/05-worker/); milestone E in the [roadmap](/resources/roadmap/).
