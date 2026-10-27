<!-- Release notes template (docs/RELEASES.md). Copy for each release; replace the <…> parts. -->
# Colophon 0.N.0 — <what this release lets you do, in one line>

Released YYYY-MM-DD. <Two to four sentences in plain words. One screenshot or a 30–90 second
screen recording (Convex attaches a walkthrough to every release).>

## Upgrade at a glance

| | |
|---|---|
| Upgrade from | 0.(N-1).x only. On anything older, upgrade to 0.(N-1) first. |
| Database migrations | 00xx to 00yy: additive only / includes a removal step |
| Downtime | none expected / stop the worker before migrating |
| Rolling back | safe to 0.(N-1) binaries / not after migration 00yy: restore your backup |
| Breaking changes | <number>, listed below |
| Guide | docs/upgrading/0.N.md |

## Breaking changes

### <Title in plain words>
- **Who is affected:** <e.g. sites that build from `/content/v1` with cursors older than 0.(N-1)>
- **What changed:** <old → new>
- **What to do before upgrading:** <steps; a query or command that tells you if you're affected>

## Deprecations

| What | Use instead | Warns since | Removed in |
|---|---|---|---|

## Highlights
<Two or three items, each a short paragraph.>

## All changes
<This version's section of CHANGELOG.md, verbatim.>

## Known issues and limits
<What doesn't work yet or misbehaves, honestly (DS1). Strapi calls this "Changes to be aware of".>

## Evidence for this release
- `scripts/check.sh` at <commit>: <passed>
- Fault cases run: F01–F0x; crash loop: <N iterations, result>
- Simulator: <N seeds, 0 failures>; load: <links to perf/results files, machine named>
- Missed or failed runs kept: <links>

## Verify the download
<checksums, signature and SBOM commands (module 19)>

## Compatibility
<generated table: server and CLI version; `/api/v1` OpenAPI file hash and status (Preview);
`/content/v1` (Stable since 0.1); rich text `colophon/rich-text` v1; CLI `--json` v1; webhook
payload v1; SDK version; PostgreSQL and Valkey versions tested; last migration number>

## Thanks
