# Proposed, not decided: custom plans on the application pool

**Status:** proposed (2026-11-17), for the maintainer. Not built.

## The problem

sqlx prepares every statement, and PostgreSQL switches a prepared statement to one generic plan
after five executions when the generic plan's estimated cost is no worse than the custom plans'
average. A generic plan can't see the parameters, so for a multi-tenant table it plans with the
average tenant: when one project grows faster than the statistics know, the plan stays the one
built for a tiny project. On 2026-11-17 that made the schedules load smoke's lists take three
seconds (run 44216694411, AMD EPYC 9V45: list p50 1.8 ms, p99 3.1 s; the logged plan removed
261,632 rows by a join filter for one list). Under a forced generic plan, a 120-row schedule list read
16,203 changeset rows and a 100-release page 10,150. Those queries are now shaped so no plan
can do that (bounded rows first, names by primary key), but the class remains: any query whose
good plan depends on per-project sizes.

## The proposal

Set `plan_cache_mode = force_custom_plan` for the application's logins (`ALTER ROLE colophon_app
SET plan_cache_mode = force_custom_plan`, or per connection in the pool's `after_connect`), so
every execution is planned with its real parameters. Measure first, and decide on the numbers:

- planning CPU per request on the load smokes (`pg_stat_statements.total_plan_time` with
  `track_planning = on`, before and after), and the smokes' p50 and p99 on the same CPU;
- the worst statements' plan time (the delivery overlay, the content query builder, the release
  list), since custom plans re-plan every call.

## Trade-offs

- Every execution pays planning again: typically 0.05–0.5 ms for these statements, more for the
  largest joins; at the delivery smoke's 1,500 requests a second that is real CPU.
- It hides the class rather than removing it per query; the query-shape rule (bounded rows
  first, then lookups) and the generic-plan row-count tests stay the first line.
- A middle course: force custom plans only for the statements known to be size-sensitive (the
  lists), via a per-transaction `SET LOCAL` in their functions — no global cost, but a list to
  keep.

## Tests it would need

A smoke pair on one runner (custom vs default) recording plan time and p99; a db test that a
list under the pool's settings plans with its parameters (`EXPLAIN` shows the literal values).
