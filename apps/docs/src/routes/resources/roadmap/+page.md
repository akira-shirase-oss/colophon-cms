<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon is built in eleven milestones, each named after what it proves. A milestone is done when its **proof gate** passes with stored evidence (a commit, a command and its raw output), not when its screens exist. Since D-057 the goal is every feature Strapi and Sanity offer, built to the same bar, so four lettered milestones (R, M, E and D) sit between the numbered ones. The estimates are guesses at about 45 hours a week (D-010), not measurements: about **9 to 12 months** to v0.1 in all, with the first public demo at the end of P3. This page follows the [full roadmap](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/ROADMAP.md) on the `r/r.2-custom-roles` branch, the newest; the website shows the same plan on its [roadmap page](site:/roadmap/).

## How to read the labels

The labels mean what they mean on the website:

- **Done**: merged into `main`, with its tests, its testbed page and its load scenario. Each done step links to its entry in the [changelog](/changelog/).
- **Building**: a step in progress on its own branch, not merged. Nothing on a branch is available from `main`.
- **Planned**: in the roadmap, nothing built.

The roadmap on the `r/r.2-custom-roles` branch marks R.1 and R.2 as done there; until they merge into `main`, they are Building here.

## Milestones at a glance

| Order | Milestone | Proof gate | Estimate | Status |
|---|---|---|---|---|
| P0 | Foundation | Editor JSON round-trips; Rust rejects invalid documents; CI is green | 1–2 weeks | Done |
| P1 | Never lose an edit | F01, F02, F04; cross-project denial; first simulator runs | 3–5 weeks | Done |
| P2 | Publish exactly once | F03, F05; concurrent releases; the simulator covers the outbox; first crash loop | 3–4 weeks | Done |
| P3 | Prove it reached the site | F06–F08, L01–L04, the mixed-release checker; first public demo | 3–4 weeks | Building |
| R | Roles and permissions | Every action, role, field and condition decided by one evaluator, with no bypass | 2–3 weeks | Building |
| M | Content modelling parity | Relations, components, lists, blocks and single types round-trip and stay valid | 4–6 weeks | Planned |
| P4 | Time travel and media | Rollback, pinned reads, revocation, upload fault tests, the media library | 3–4 weeks | Planned |
| E | Editorial parity | Localization, scheduling, review stages, comments, tasks and live collaboration without a lost edit | 5–7 weeks | Planned |
| D | Developer parity | Query API, GraphQL, SDK, API tokens, import from Strapi and Sanity, extension points | 4–5 weeks | Planned |
| P5 | Safe automation | Zero unauthorized actions in the evaluation set; an interrupted bulk job resumes | 3–4 weeks | Planned |
| P6 | Operate it | F09–F14, L05–L13, a 24-hour soak, a restore drill; **v0.1** | 3–4 weeks | Planned |

## P0 · Foundation

**Proof gate:** valid rich-text fixtures round-trip through the editor, JSON, the Rust validator and the renderer; invalid ones are refused with field paths; CI is green from a clean clone. Met on 2026-10-25. <Status done="P0" />

- **0.1** Pinned toolchain, workspace and CI. [Changelog](/changelog/#step-0-1) <Status done="0.1" />
- **0.2** The local stack. [Changelog](/changelog/#step-0-2) <Status done="0.2" />
- **0.3** Errors, honest health, the testbed and load tests. [Changelog](/changelog/#step-0-3) <Status done="0.3" />
- **0.4** The rich-text format, shared fixtures and the editor spike. [Changelog](/changelog/#step-0-4) <Status done="0.4" />
- **0.5** The Rust rich-text validator. [Changelog](/changelog/#step-0-5) <Status done="0.5" />
- **0.6** The renderer. [Changelog](/changelog/#step-0-6) <Status done="0.6" />
- **0.7** Material 3 and the Studio's first screen; the look was replaced in 3.5. [Changelog](/changelog/#step-0-7) <Status done="0.7" />

Release notes: [P0](/releases/p0/).

## P1 · Never lose an edit

**Proof gate:** F01 (killed before commit: no false acknowledgement), F02 (killed after commit: the retry gets the same answer) and F04 (two editors on one version: one conflicts, nothing is lost); a second project denied on every route; the first simulator runs. Re-checked and met after step 1.8, on 2026-10-26. <Status done="P1" />

- **1.1** Signing in, and projects that can't see each other. [Changelog](/changelog/#step-1-1) <Status done="1.1" />
- **1.2** A draft that can't be lost. [Changelog](/changelog/#step-1-2) <Status done="1.2" />
- **1.3** Two editors, one article. [Changelog](/changelog/#step-1-3) <Status done="1.3" />
- **1.4** The first simulator run. [Changelog](/changelog/#step-1-4) <Status done="1.4" />
- **1.5** Content types you define. [Changelog](/changelog/#step-1-5) <Status done="1.5" />
- **1.6** The command line saves the same draft. [Changelog](/changelog/#step-1-6) <Status done="1.6" />
- **1.7** Inviting people. [Changelog](/changelog/#step-1-7) <Status done="1.7" />
- **1.8** The first owner. [Changelog](/changelog/#step-1-8) <Status done="1.8" />

Carried forward, not built in P1: URL names (step 3.6), locales (milestone E), environments other than `production`, and the generated OpenAPI document (D-027). Release notes: [P1](/releases/p1/).

## P2 · Publish exactly once

**Proof gate:** F03 (the worker killed after announcing: the retry has no duplicated effect), F05 (an old delivery after a newer release is superseded); two releases on one base can't both commit; the simulator covers the outbox; the first real-process crash loop. Met with step 2.4, on 2026-10-26. <Status done="P2" />

- **2.1** Approving the exact change. [Changelog](/changelog/#step-2-1) <Status done="2.1" />
- **2.2** One release, one number. [Changelog](/changelog/#step-2-2) <Status done="2.2" />
- **2.3** Work that survives a crash. [Changelog](/changelog/#step-2-3) <Status done="2.3" />
- **2.4** Killing it on purpose. [Changelog](/changelog/#step-2-4) <Status done="2.4" />

Release notes: [P2](/releases/p2/).

## P3 · Prove it reached the site

**Proof gate:** F06 (a new matching article invalidates listings), F07 (unpublished content is denied at the origin), F08 (losing the cache loses nothing), L01 to L04, and the mixed-release checker; then the first public demo, a recorded crash loop and a live site with receipts. F06 to F08 and L04 pass; the rest is still to come. <Status building="P3" />

- **3.1** A cache that can't serve the wrong thing. [Changelog](/changelog/#step-3-1) <Status done="3.1" />
- **3.2** Taking it back: unpublishing and CDN purges. [Changelog](/changelog/#step-3-2) <Status done="3.2" />
- **3.3** A receipt for every publish. [Changelog](/changelog/#step-3-3) <Status done="3.3" />
- **3.4** My own site, running on Colophon, locally. [Changelog](/changelog/#step-3-4) <Status done="3.4" />
- **3.5** The Studio, designed. [Changelog](/changelog/#step-3-5) <Status done="3.5" />
- **3.6** Content types and URL names: the content-type builder, URL names unique among drafts and among what is published, and changes checked against stored content (D-054, D-055). Branch `p3/3.6-modelling`. [Changelog](/changelog/#step-3-6) <Status building="3.6" />
- **3.7** The non-technical path (D-059). Branch `p3/3.7-onboarding`, not pushed: so far, setting up a fresh installation from a browser with a one-time code. Plain words from one glossary, one-click publishing, a first-run checklist and phone-friendly review are still to come. [Changelog](/changelog/#step-3-7) <Status building="3.7" />
- **3.8** My site goes live, and the first public demo: the docs skeleton, the simulator's cache and delivery faults, the mixed-release checker, a recorded crash loop. <Status planned="P3" />

Two fixes are merged into `main` between steps: the Studio reads your role from the project, not from your list of projects ([changelog](/changelog/#step-fix)), and title search no longer walks the whole collection ([changelog](/changelog/#step-fix-3-5)).

Release notes so far: [P3](/releases/p3/).

## R · Roles and permissions

**Proof gate:** one evaluator decides every action for every role, field and condition, exhaustively for the built-in roles and property-tested for random ones; every route has a denial test; a token can do exactly what its permissions allow; a load scenario with custom roles shows no regression (D-058). <Status building="R" />

- **R.1** Roles and permissions as data, behaviour unchanged. Branch `r/r.1-roles-as-data`. [Changelog](/changelog/#step-r-1) <Status building="R.1" />
- **R.2** Roles you define, and the Studio's role editor; nobody hands out more than they hold. Branch `r/r.2-custom-roles`. [Changelog](/changelog/#step-r-2) <Status building="R.2" />
- **R.3** Field-level permissions and "only their own" conditions. <Status planned="R" />
- **R.4** API tokens and the audit log. <Status planned="R" />

## M · Content modelling parity

**Proof gate:** every field type round-trips byte for byte; references never dangle, enforced by the database; a migration that fails halfway resumes without duplicates; the simulator covers schema changes racing saves. <Status planned="M" />

What it plans: references, nested groups of fields, lists and block lists, single types, more field types (email, URL, date, time, JSON, Markdown, colour), descriptions, defaults and conditional fields, and migrations with a plan, a dry run and an approval for the changes D-055 refuses. Its steps aren't numbered yet.

## P4 · Time travel and media

**Proof gate:** rollback and pinned-read tests; revoked content stays unavailable through historical releases; malicious uploads are refused; L14 (large media keeps editing responsive); X07 (daylight saving and expired approvals in scheduling). <Status planned="P4" />

What it plans: rollback as a new release, release history and comparison in the Studio, restoring a version, the media library with safe uploads, previews on a separate origin, and sitemaps and feeds from one release. Its steps aren't numbered yet.

## E · Editorial parity

**Proof gate:** two editors editing at once never lose an acknowledged edit; a schedule missed during downtime is reported as delayed, never on time; locale variants publish independently; comments and tasks follow permissions. <Status planned="E" />

- Scheduled publishing, built ahead of the rest of E: schedule an approved publish request for a local time in a time zone, with daylight-saving edges never guessed. Branch `e/schedules`, not merged. [Changelog](/changelog/#step-schedules) <Status building="e/schedules" />
- Localization, review stages, comments, tasks, live presence and collaboration (O-017), live queries, and visual preview. <Status planned="E" />

## D · Developer parity

**Proof gate:** every query is permission-filtered; GraphQL and REST answer the same; imports of the Strapi and Sanity reference datasets round-trip; load scenarios for queries and GraphQL. <Status planned="D" />

What it plans: a query API with filters, sorting and field selection, GraphQL, the OpenAPI document and the TypeScript SDK, webhooks by event, import and export, and extension points instead of plugins (custom field types as data, Studio panels in sandboxed frames). Its steps aren't numbered yet.

## P5 · Safe automation

**Proof gate:** zero unauthorized actions across the evaluation set; X12 (an agent interrupted halfway through a bulk edit resumes without duplicates or overwrites); L08 (a noisy agent is throttled while editors keep their latency). <Status planned="P5" />

What it plans: machine credentials and a kill switch, typed tools and bulk manifests with per-item outcomes, budgets reserved in PostgreSQL, the MCP adapter, the CLI's full JSON contract, import and export. Its steps aren't numbered yet.

## P6 · Operate it

**Proof gate:** F09 to F14, L05 to L13, a 24-hour soak, a restore drill on clean infrastructure, the five-editor usability test and one independent install; then **v0.1**. <Status planned="P6" />

What it plans: encrypted backups and restore (see [backups](/operations/backups/)), the upgrade runbook, observability, the full fault and load matrix, complete guides with every example run in CI, and the release process with signed artifacts and an SBOM. Its steps aren't numbered yet.

## Alongside the milestones

- **The website**, with the product, the comparison, the roadmap, the changelog and the build log. Branch `site/new-look`. [Changelog](/changelog/#step-site) <Status building="site/new-look" />
- **These docs**, with the changelog, release notes and upgrade guides. Branch `site/docs`. [Changelog](/changelog/#step-docs) <Status building="site/docs" />

## What v0.1 means

Version 0.1 is done when every one of these has stored evidence ([end goal](/design/end-goal/)). None is checked yet.

- The maintainer's own site runs on Colophon in production.
- A recorded demo of the crash loop: random `kill -9` of the server and worker during saves and publishes, then a checker proving nothing accepted was lost, no release ran twice and no reader saw a mixed release.
- The deterministic simulator runs in CI on every pull request, any failing seed replays exactly, and a deliberately broken rule is caught.
- Every fault case F01 to F14 and load case L01 to L14 passes.
- A benchmark report on the reference profile, with commands, dataset seed and raw results.
- A backup-and-restore drill onto clean infrastructure.
- One person other than the maintainer installs Colophon from the README alone, and the friction is written up.
- Five editors try draft, preview, review and publish; at least four succeed without help.
- A release with signed artifacts, an SBOM, a changelog, this documentation site, and a technical write-up for each hard problem.

## Following along

Each step is written up, mistakes included, in the build log at [404human.dev/colophon](https://404human.dev/colophon/). The [decisions](/design/decisions/) record every place Colophon deliberately departs from its original specification, and why; D-054 and the decisions after it are on branches until those merge.
