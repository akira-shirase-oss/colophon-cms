# Changelog

Every step of Colophon CMS merged into `main`, newest first. Colophon is pre-release: v0.1 is not out, there are no
version numbers yet, and nothing here is software you can install from a release. Each entry is one step of a
milestone in [docs/ROADMAP.md](docs/ROADMAP.md), written from the repository's own history: the merge commits
(`git log --merges main`), the step's commits, and the decisions in [docs/DECISIONS.md](docs/DECISIONS.md). The
build log at <https://404human.dev/colophon/> tells the same story in prose, mistakes included.

The public website renders this file at build time (`apps/site`), so this file is the only place an entry is written.

## Unreleased

### 3.6 · Content modelling in the Studio

- **Status:** building
- **Branch:** `p3/3.6-modelling`
- **Decisions:** D-054, D-055 (on the branch, not merged)

Nothing from this step is on `main` yet; the branch holds work in progress that doesn't compile yet (`10f3c65`). What it
is building:

- A **slug field type** (a "URL name" in the Studio): 1 to 120 lowercase letters and digits joined by single hyphens,
  refused rather than rewritten when it isn't one, and unique per collection among drafts and among what is published,
  enforced by two partial unique indexes in PostgreSQL (D-054).
- A **content-type builder whose changes are checked against stored content**: removing a field, making one required
  or narrowing a limit is allowed when every draft and every published version still fits, and refused otherwise with
  the first 20 entries in the way (D-055).
- The branch also carries the scope decisions (`63e82cb`): every Strapi and Sanity feature at Colophon's bar of proof
  (D-057), roles and permissions as data (D-058), non-technical editors first (D-059), and `docs/COMPARISON.md`.

### 3.5 · The Studio, designed

- **Status:** merging
- **Branch:** `p3/3.5-design`
- **Decisions:** D-053, D-056

- Title search, a live state on entry lists, revision reads and review diffs in the API and the Studio.
- The design system: every shadcn-svelte component, owned in `packages/ui` and restyled on Colophon's own tokens
  (D-053), then given its current look of quiet paper, black ink and one warm accent (D-056). Inter only, self-hosted.
- Every Studio screen rebuilt on it, a Colophon sign-in theme for the local Keycloak, and `/studio/design`, a page
  with every token and component.
- A contrast test computed from the token values: 124 text-on-ground checks pass at WCAG AA, and AAA under the
  system's "more contrast" setting.
- Two entries load runs that missed the regression gate are kept as evidence, and one finding: on a local database
  grown to 5 GB, saves ran at 1,149/s with a p99 of 183.8 ms, against 1,785/s and 52.4 ms on a fresh one.

## P3 · Prove it reached the site

### 3.4 · My own site, running on Colophon

- **Merged:** 2026-10-26 · `0864e22`, `0550292`
- **Decisions:** D-052
- **Build log:** <https://404human.dev/colophon/checkpoint-3-4/>

- `apps/web`, the reference site: a static build that reads exactly one release, with every read pinned to it, and
  renders with the same renderer as the Studio's preview.
- Every page carries the release it was built from, exactly once, as `<meta name="colophon-generation">`.
- A build is checked, then promoted with one atomic rename; an older release can never replace a newer one, and a
  failed build leaves the live site alone.
- A runner rebuilds on each release's webhook; `scripts/demo.sh` runs the CMS, the Studio and the site locally.
- Load: 1,000,195 page reads at 19,013/s (p99 6.9 ms) while 27 releases were published underneath the readers.
- Runs locally only; putting the site on the internet is a later step.

### 3.3 · A receipt for every publish

- **Merged:** 2026-10-26 · `c5980d6`
- **Decisions:** D-051
- **Build log:** <https://404human.dev/colophon/checkpoint-3-3/>

- Probes: a destination that fetches a URL and looks for the release's generation marker, keeping every observation
  (what it looked for, from where, what it saw, the body's SHA-256).
- Every release gets one label: *Published in CMS*, *Website update pending*, *Verified at N locations*, or *Needs
  attention*. "Verified" always says at how many locations.
- Labels and receipts in the Studio, the testbed and the load scenarios.
- Two million-request publishing runs that failed (a statement timeout in teardown, since fixed) are kept, next to
  the passing one: 1,000,393 requests at 941/s, p99 201 ms.

### 3.2 · Taking it back

- **Merged:** 2026-10-26 · `f15b0d7`
- **Decisions:** D-049, D-050
- **Build log:** <https://404human.dev/colophon/checkpoint-3-2/>

- Unpublishing goes through review like any other change; its release leaves a tombstone and revokes every earlier
  publication, so the origin refuses the entry at any release, historical URLs included.
- A destination can be a CDN: each release's job is a purge by tag covering every release since the last successful
  purge, and each release reports whether its purge was accepted.
- Load: publishing with takedowns and CDN purges, 1,000,328 requests at 912/s, p99 145 ms, none failed.

### 3.1 · A cache that can't serve the wrong thing

- **Merged:** 2026-10-26 · `e5c874a`
- **Decisions:** D-048
- **Build log:** <https://404human.dev/colophon/checkpoint-3-1/>

- Public answers are cached in Valkey under keys that name their release generation; the generation is always read
  from PostgreSQL, in one statement.
- Misses for one key share one fill; the cache is disposable, and when it is down reads fall back to PostgreSQL.
- Load: 1,000,058 requests at 6,340/s with a p99 of 25 ms, 99.99% of answers without a database read of their own
  (2,650/s and 31 ms before the cache).

### Fix · The Studio reads a project's role from the project

- **Merged:** 2026-10-26 · `0512497`

- The Studio read your role in a project from your list of projects; it now reads it from the project itself.

## P2 · Publish exactly once

### 2.4 · Killing it on purpose

- **Merged:** 2026-10-26 · `c38ec47`
- **Decisions:** D-047
- **Build log:** <https://404human.dev/colophon/checkpoint-2-4/>

- The simulator's publishing scenario (`colophon-sim --scenario publishing`): publishers racing on the generation they
  saw, an outbox, leased workers that crash at any step, and a network that loses, duplicates and refuses.
- Eight planted bugs, each of which the simulator must catch; CI sweeps 1,000 seeds.
- Modelling the worker found a real gap: two announcements could be in flight to one destination at once, so a slow
  network could deliver release N after N+1. Fixed: one announcement in flight per destination (migration 0010).
- A real-process crash loop (`tests/crash_loop.rs`) kills the server and worker with SIGKILL at random moments, twelve
  times per CI run, and checks that nothing acknowledged was lost or doubled.
- P2's proof gate is met.

### 2.3 · Work that survives a crash

- **Merged:** 2026-10-26 · `070415e`
- **Decisions:** D-046
- **Build log:** <https://404human.dev/colophon/checkpoint-2-3/>

- Releases are announced through a transactional outbox, by a worker (`crates/colophon-worker`) that claims jobs under
  leases and posts with a stable idempotency key, to public addresses only.
- Crash tests F03 (the worker killed after announcing: the next attempt sends again with the same key) and F05 (a
  release superseded by a newer one is never sent).
- The worker got its own database pool after the first publishing load runs missed their targets; those results are
  kept.

### 2.2 · One release, one number

- **Merged:** 2026-10-26 · `2831ef2`
- **Decisions:** D-045
- **Build log:** <https://404human.dev/colophon/checkpoint-2-2/>

- Publishing commits one release, in one transaction, on the generation the publisher saw; two releases on the same
  base can't both commit.
- Publication history is non-overlapping intervals, so any past release can be read exactly.
- `/content/v1` serves a published copy through a database role that can't read drafts.
- Load: 1,000,058 anonymous reads at 2,650/s, p99 31 ms.

### 2.1 · Approving the exact change

- **Merged:** 2026-10-26 · `ec7a3c2`
- **Decisions:** D-044
- **Build log:** <https://404human.dev/colophon/checkpoint-2-1/>

- Changesets (publish requests) pin the exact revisions of the entries in them, and their hash is computed from those
  revisions.
- A review counts only while the changeset keeps the hash it was given on: edit anything afterwards and it needs
  approving again.
- Studio pages to pick entries, submit, and approve the version on screen.

## P1 · Never lose an edit

### 1.8 · The first owner

- **Merged:** 2026-10-26 · `2652f43`
- **Decisions:** D-043
- **Build log:** <https://404human.dev/colophon/checkpoint-1-8/>

- `colophon admin bootstrap` makes the first administrator once, from the operator's terminal; only administrators
  create projects.
- P1's proof gate re-checked and met: F01 and F02 against the real binary, F04 in two browsers, a second project
  denied on every route.

### 1.7 · Inviting people

- **Merged:** 2026-10-25 · `76aac72`
- **Decisions:** D-042
- **Build log:** <https://404human.dev/colophon/checkpoint-1-7/>

- One policy table decides what owners, developers, editors and publishers may do.
- Owners invite people with single-use links; a project can never lose its last owner.
- Fixed before merging: an owner demoted mid-request could still manage members.

### 1.6 · The command line saves the same draft

- **Merged:** 2026-10-25 · `6a7157b`
- **Decisions:** D-041
- **Build log:** <https://404human.dev/colophon/checkpoint-1-6/>

- `colophon login`, `logout`, `whoami`, `content create` and `content list`.
- The CLI signs in with a code you approve in the Studio (`/studio/device`) and gets a personal bearer token; bearer
  tokens work on every route.

### 1.5 · Content types you define

- **Merged:** 2026-10-25 · `2254117`
- **Decisions:** D-040
- **Build log:** <https://404human.dev/colophon/checkpoint-1-5/>

- Content types are versioned definitions in PostgreSQL with six field types: text, rich text, number, boolean,
  choice and date-time.
- Only changes that keep every stored entry valid are allowed; every revision records the definition version it was
  validated against.
- The Studio's editor is driven by the type, field by field; `articles` became an ordinary type.

### 1.4 · The first simulator run

- **Merged:** 2026-10-25 · `542d0eb`
- **Decisions:** D-039
- **Build log:** <https://404human.dev/colophon/checkpoint-1-4/>

- `crates/colophon-sim`: a seeded, single-threaded, dependency-free simulator of colliding editors, lost answers and
  crashes; a failing run replays exactly from its seed.
- A differential test replays simulated runs against PostgreSQL and requires the same answers, save for save.
- It found a real double-save case in the Studio: an unanswered save is now resent with its key after a reload.

### 1.3 · Two editors, one article

- **Merged:** 2026-10-25 · `41733b0`
- **Decisions:** D-037, D-038
- **Build log:** <https://404human.dev/colophon/checkpoint-1-3/>

- The server serves the Studio at `/studio`, on its own origin.
- The Studio edits real drafts with autosave, a copy of unsaved edits on the device, and a conflict screen that shows
  both versions instead of overwriting anyone (F04 end to end).

### 1.2 · A draft that can't be lost

- **Merged:** 2026-10-25 · `00cfba4`
- **Decisions:** D-034, D-035, D-036
- **Build log:** <https://404human.dev/colophon/checkpoint-1-2/>

- Entries with insert-only revisions; a save names the revision it was made on (`If-Match`), and a stale save is
  refused with 409.
- Idempotent writes: a retried save returns the same result instead of saving twice.
- Crash tests F01 (killed before commit: no false acknowledgement) and F02 (killed after commit: the retry gets the
  same answer), through fault points compiled into a test build.

### 1.1 · Signing in, and projects that can't see each other

- **Merged:** 2026-10-25 · `397cb59`
- **Decisions:** D-031, D-032, D-033
- **Build log:** <https://404human.dev/colophon/checkpoint-1-1/>

- Sign-in through OpenID Connect (Keycloak locally), server-side sessions, CSRF tokens and exact `Origin` checks.
- Projects isolated by PostgreSQL row-level security.
- A full database pool answers 503 with `Retry-After` instead of an internal error.

## P0 · Foundation

### 0.7 · Material 3 and the Studio's first screen

- **Merged:** 2026-10-25 · `1455073`
- **Decisions:** D-024 to D-030
- **Build log:** <https://404human.dev/colophon/checkpoint-0-7/>

- `packages/ui` and the Studio's first screen on Material 3 tokens (since superseded by D-053 and D-056).
- 116 contrast checks, and axe found no WCAG 2.1 AA violations in four theme and contrast combinations.
- P0 is done.

### 0.6 · The renderer

- **Merged:** 2026-10-25 · `c195f4a`
- **Decisions:** D-023
- **Build log:** <https://404human.dev/colophon/checkpoint-0-6/>

- `packages/content-renderer`: one renderer for the Studio's preview and the public site, building every tag itself
  and escaping every piece of text.
- The reviewed HTML for every valid fixture became part of the shared contract.

### 0.5 · The Rust rich-text validator

- **Merged:** 2026-10-25 · `906a19e`
- **Build log:** <https://404human.dev/colophon/checkpoint-0-5/>

- `colophon-domain` validates rich text in one pass and writes its canonical form, the exact bytes to store; anything
  else is refused with a stable code and a JSON Pointer.
- Every valid fixture is accepted byte for byte as the editor writes it; every invalid one is refused with exactly its
  code and pointer.
- `POST /api/v1/rich-text/validate` and its load scenario: 1,000,000 requests at 71,270/s, p99 2.28 ms.

### 0.4 · The rich-text format, shared fixtures and the editor spike

- **Merged:** 2026-10-25 · `eb742b4`
- **Decisions:** D-021, D-022
- **Build log:** <https://404human.dev/colophon/checkpoint-0-4/>

- `colophon/rich-text` v1: Tiptap JSON behind an allowlist, with limits, a canonical form and error codes.
- Shared fixtures of valid and invalid documents, used by every implementation.
- `packages/editor` and the Studio's editor spike: a document opens only if the editor can give it back byte for byte.

### 0.3 · Errors, honest health, the testbed and load tests

- **Merged:** 2026-10-25 · `8d0f80f`
- **Decisions:** D-018, D-019, D-020
- **Build log:** <https://404human.dev/colophon/checkpoint-0-3/>

- One JSON error envelope with a stable code and the request id, `/livez`, `/readyz` and `colophon doctor`.
- The testing rule (D-018): every feature gets regression tests, a page in `apps/testbed`, and a load scenario whose
  results are committed, failures included.
- Load: 5,000,000 health requests at 75,405/s, p99 1.64 ms.

### 0.2 · The local stack

- **Merged:** 2026-10-25 · `6994073`
- **Decisions:** D-015, D-016, D-017
- **Build log:** <https://404human.dev/colophon/checkpoint-0-2/>

- `deploy/compose.yaml`: PostgreSQL 18.6, Valkey 9.0.6, Garage 2.5.0 and Keycloak 26.8.1, pinned by digest, every
  port on `127.0.0.1`.
- `scripts/stack.sh up` runs 28 checks through the published ports, and makes each service refuse something it must
  refuse.

### 0.1 · Pinned toolchain, workspace and CI

- **Merged:** 2026-10-25 · `68f471b`
- **Decisions:** D-013, D-014
- **Build log:** <https://404human.dev/colophon/checkpoint-0-1/>

- Rust 1.99.1 pinned, a five-crate workspace, and a check that fails when the compiler on `PATH` ignores the pin.
- `scripts/check.sh` runs exactly what CI runs; CI pins every third-party action to a commit.
