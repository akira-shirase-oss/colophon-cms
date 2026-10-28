# Changelog

Every change to Colophon CMS that someone could notice, newest first. The format follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/), adapted for a project that hasn't released anything
yet: Colophon is pre-release, v0.1 is not out, and nothing here is software you can install from a release. Until
v0.1 there are no version numbers, so each step merged into `main` is one entry, grouped by the milestone of
[docs/ROADMAP.md](docs/ROADMAP.md) it belongs to, the latest milestone first and each milestone's steps newest first.
**Unreleased** lists the work on branches: none of it is merged. At v0.1, version sections begin above this step
history ([docs/RELEASES.md](docs/RELEASES.md)).

Each entry gives its merge date and merge commits (`git log --merges main`), the decisions behind it
([docs/DECISIONS.md](docs/DECISIONS.md)), its build-log post, and the proof behind any guarantee it states: fault
cases, tests and load results. Its changes are grouped as Added, Changed, Deprecated, Removed, Fixed and Security;
each starts with its area and says first, in plain words, what you can now do, then the technical detail. The plain
words are the Studio's (D-059): a **publish request** is what the API calls a changeset, a **version** a revision, a
**URL name** a slug, and **website connections** are destinations; the Studio has used them on every screen since step
3.7. Load figures come from runs on one laptop, an Apple M2 Pro with 12 cores (`apple-m2-pro-12c`),
with every result kept in `perf/results/`, misses included: they are evidence about the code, not a promise about
production hardware. The build log at [404human.dev/colophon](https://404human.dev/colophon/) tells the same story
in prose, mistakes included.

This file is the only place an entry is written: the website and the docs render it at build time.

## Unreleased

### Docs · The documentation site

- **Status:** building
- **Branch:** `site/docs`

Nothing from this work is merged. The documentation site is being built and moved onto the website's look.

#### Added

- **Docs:** Guides for editors in plain words, a quickstart that uses only commands the repository has, concept pages
  from drafts to the simulator, and references for the API, every route, every error code, the delivery API, the
  CLI, the rich-text format and webhooks; each page says whether what it describes is done, being built or planned.
  Technical: `apps/docs`, SvelteKit with every page prerendered, Markdown through mdsvex, Pagefind search on the same
  origin, nothing loaded from another origin.
- **Docs:** The references are checked against the code: a test fails when a route (`lib.rs`), an error code
  (`error.rs`) or a CLI command (`main.rs`) is missing, and every command marked runnable must exist in the
  repository. The design documents and every load result are rendered from the repository's own files.
- **Docs:** This changelog at `/docs/changelog/` with an Atom feed, release notes for each milestone
  (`docs/releases/`), upgrade guides (`docs/upgrading/`), and a roadmap page with every step's status.
- **Docs:** How Colophon tells people what changed ([docs/RELEASES.md](docs/RELEASES.md)), from a study of eight
  platforms, with templates for release notes and upgrade guides (`docs/templates/`).
- **Tests:** Playwright tests on the built site (108 when first committed): pages, links and anchors, accessibility
  in light and dark, search and runnable commands, run by `npm run test -w @colophon/docs`.
- **Docs:** Pages for the work below that is built on branches: a content modelling reference with every field type,
  its stored form and a worked example; API tokens, the audit log and `GET {P}/permissions`; the MCP server; and
  `GET /version`. Each is labelled Building. Technical: they describe the code at `m/m4-validation` (`c127feb`),
  which this branch doesn't carry yet, so `routes.spec.ts` passes with `COLOPHON_DOCS_CODE_REF` set to it and fails
  on this branch's own router until that code is merged here.

### Site · The new website

- **Status:** building
- **Branch:** `site/new-look`

Nothing from this work is merged. The public website is built and tested on its branch; it has no host yet (O-012).

#### Added

- **Site:** A public website that says what Colophon is, what is done, being built or planned, how it compares with
  Strapi and Sanity, the roadmap, this changelog and the build log. Technical: `apps/site`, SvelteKit with the static
  adapter, every page prerendered, nothing loaded from another origin; `/blog/rss.xml`, `/sitemap.xml` and
  `/robots.txt` prerendered.
- **Site:** Nothing on it can drift from the repository: the changelog is read from this file, the blog's posts are
  byte-for-byte copies of the build log's, and every load figure is read from `perf/results/` with its file, machine
  and the laptop caveat; a missing file or a malformed entry fails the build. Every feature carries exactly one
  status, the licence is described as proposed, not confirmed (O-014), and a quote must match its source word for
  word.
- **Site:** The layout, type and motion are measured from a reference layout at four widths, with Colophon's own
  content, illustrations of the Studio's real states, and live graphics of real behaviour (a publish played out to
  *Verified at 1 location*). Under reduced motion, or without JavaScript, every graphic shows its final frame. Light
  and dark switch with one button.
- **Tests:** `apps/site/tests/site.spec.ts`, `motion.spec.ts` and `look.spec.ts`: every page with one h1, no console
  errors and no axe violations in light and dark, no request to another origin, the changelog equal to this file,
  every feature's status, the licence's wording, no sideways scrolling at 320px, and the motion rules.
- **Docs:** [Module 20](https://github.com/akira-shirase-oss/colophon-cms/blob/site/new-look/docs/modules/20-site.md)
  describes the site, where its numbers came from, and its invariants W1 to W9.

### M.4 · Single types, patterns and unique fields

- **Status:** building
- **Branch:** `m/m4-validation`
- **Decisions:** D-067, D-069, D-070
- **Proof:** `crates/colophon-domain/src/schema.rs` (patterns matched whole, `(a+)+` against 50,000 characters finishing; unique only where it may be), `crates/colophon-server/tests/singles.rs` (two creations racing, one wins), `unique.rs` (exact comparison, drafts against publications, a type change refused then accepted, releases, who is named, eight racing creates leaving one entry) and `content_types.rs` (a pattern checked against stored content); load smokes only

Nothing from this work is merged (single types were built on `m/m4-single-types`), and M.4 isn't finished: shared
components, defaults and conditional fields are still to come. The Studio's builder doesn't offer these options yet;
they are set through the API.

#### Added

- **Content types:** A content type can hold a single entry, such as a site's settings or its home page: a second is
  refused, however many people create one at once. Technical: `"single": true` in the definition, written only when
  true; a partial unique index, `entries_single` (migration 0026); `409 single_exists`; turning it on is refused with
  `422 too_many_entries` at `/single` while the collection has more than one entry.
- **Content types:** A text field can require an allowed form, such as three capital letters, a hyphen and four
  digits. Technical: `pattern`, 1 to 500 characters of Rust `regex` syntax matched against the whole value in linear
  time, without look-around or back-references, where `\d` is Unicode-aware (use `[0-9]` for ASCII digits);
  `422 invalid_pattern` where it is defined and `pattern_mismatch` at the value; adding or changing one is checked
  against stored content, removing one never is.
- **Content types:** A field can be unique, such as an email address or a product code: no two entries share a value
  among drafts, and none among what is published. Technical: `"unique": true` on a top-level single-line text, email,
  URL, number, date, time or date-time field (`invalid_unique` elsewhere); `unique_claims` and its unique constraint
  (migration 0027), values compared exactly as stored; `409 value_taken` on a save or a release; making a field unique
  is refused with `needs_migration` while two drafts or two publications share a value.

#### Security

- **API:** `slug_taken` and `value_taken` name the entry in the way only to a role that may read its title; before,
  `slug_taken` named it to any role that could write, including one limited to its own entries.

### P5 · The MCP server

- **Status:** building
- **Branch:** `p5/mcp`
- **Decisions:** D-068
- **Proof:** `cargo test -p colophon-mcp` (framing, arguments, refusals, configuration) and `crates/colophon-mcp/tests/stack.rs` against the local stack with a custom, a read-only and a full-access token

Nothing from this work is merged or published. It is P5's first slice; P5's proof gate isn't met.

#### Added

- **Agents:** An AI agent can read content, write drafts and ask people to review a publish request through the Model
  Context Protocol, with a project API token; it can never approve, publish, take down or schedule anything, or
  manage roles, members or tokens. Technical: `crates/colophon-mcp`, local stdio, MCP 2025-11-25 on a JSON-RPC layer of
  its own; eight tools over `/api/v1`; tool discovery from `GET {P}/permissions`; writes from the revision and hash
  read, with idempotency keys; refusals kept whole as tool errors; bounded messages, answers, calls and concurrency.
  Built from source only.

### V · One version, and packages not yet published

- **Status:** building
- **Branch:** `v/versions-distribution`
- **Decisions:** D-062

Nothing from this work is merged, and nothing has been published to npm, Cargo or Homebrew: publishing waits on the
maintainer (an npm organization, a publishing token, a confirmed licence and a release tag).

#### Added

- **CLI:** The command line says in plain words when it is too old or too new for the server it talks to, and
  `colophon --version` names the version, commit and build date. Technical: one version, `0.1.0-alpha.1`, for every
  crate and package; `GET /version` (unauthenticated, `no-store`, a shape that only grows).
- **Operations:** The pieces for installing without building Rust: the `@colophon-cms/cli` npm launcher with one
  package per platform, an install script that checks before it installs, and a Homebrew formula template. Technical:
  a release workflow for five targets with checksums, SBOMs and signed provenance, written but not yet run.

### M.3 · Objects, lists and page-builder blocks

- **Status:** building
- **Branch:** `m/m3-structures`
- **Decisions:** D-066
- **Proof:** `crates/colophon-domain/src/schema.rs` (structures, their depth and size, and changes inside them), `crates/colophon-server/tests/references.rs` (a reference inside a page-builder block); `perf/k6/modelling.js`, smokes only so far

Nothing from this work is merged; the million-request runs of `perf/k6/modelling.js` are still to be made.

#### Added

- **Content types:** Fields inside fields, lists of values, and page builders whose items are blocks of the kinds you
  offer. Technical: `object`, `list` (`of`, at most 100 items) and `blocks` (1 to 20 kinds, items marked `_block`,
  written first); three deep at most (`too_deep`); a reference inside keeps its path (`sections/1/by`, migration
  0025).

### M.2 · References that never dangle

- **Status:** building
- **Branch:** `m/m2-references`
- **Decisions:** D-065
- **Proof:** `crates/colophon-server/tests/references.rs` (`readers_never_get_a_reference_to_an_entry_they_cannot_get`)

Nothing from this work is merged.

#### Added

- **Content types:** A field can name other entries, and Colophon makes sure they exist: on every save, and on every
  publish, so readers never get a reference to an entry they can't read. Reading an entry says what references it.
  Technical: `reference` (`to`, `many`, `max` up to 100); `revision_references` with foreign keys (migration 0024);
  `422 reference_not_found`; `409 references_unpublished` and `still_referenced` on publish; `referenced_by` on a read.

### M.1 · Seven more field types

- **Status:** building
- **Branch:** `m/m1-field-types`
- **Decisions:** D-064

Nothing from this work is merged.

#### Added

- **Content types:** Email, web address, date, time, JSON, Markdown and colour fields, help text on any field, and a
  minimum length on text. Technical: `email`, `url` (stored in its WHATWG form), `date`, `time`, `json` (keys sorted,
  64 KiB), `markdown` and `color` (`#rrggbb`, lowercase); `description` and `min_chars`, written only when set, so
  older definitions keep their hashes.

### R.4 · API tokens and the audit log

- **Status:** building
- **Branch:** `r/r.4-tokens-audit`
- **Decisions:** D-063
- **Proof:** `crates/colophon-server/tests/tokens.rs` (a token does exactly what its permissions allow and stops when revoked; every change to access is in the log, which only owners read), `crates/colophon-db/tests/tokens.rs` (the log refuses even its owner; a revocation waits for a running request); `perf/k6/tokens.js`, smokes only so far

Nothing from this work is merged; the million-request runs of `perf/k6/tokens.js` are still to be made, so R.4 isn't
done. The Studio has no page for tokens or the log yet.

#### Added

- **API:** Owners can give a script, a build or an agent its own project API token: read-only, full access or a
  custom set of permissions, expiring within 366 days, its secret shown once. What it writes names the token, and
  revoking it stops it at once. Technical: `/api/v1/projects/{project}/tokens`; `colophon_api_` secrets stored as
  SHA-256 hashes (migration 0023); no token may manage members, roles or tokens or read the log (`not_for_tokens`);
  only its project's `{P}` routes, `api_token_not_allowed` elsewhere and for scheduling; a share lock per request
  makes revocation exact.
- **API:** Owners can read who changed roles, members, invitations and tokens, and when; nobody can change or delete
  the record. Technical: `audit_events`, written in the same transaction as each change, with a trigger refusing
  `UPDATE`, `DELETE` and `TRUNCATE` from anyone; `GET /api/v1/projects/{project}/audit` with `audit.read`.
- **API:** Anyone can ask what they may do in a project. Technical: `GET {P}/permissions`, for people and tokens.

### API · Every refusal names its reason

- **Status:** building
- **Branch:** `api/dashboard-fields`

Nothing from this work is merged.

#### Changed

- **API:** Every `403` and `409` says why in a stable code (`missing_permission` with the permission, `cannot_grant`,
  `last_owner`, `already_published` and more), and an entry's read says whether it is published.
- **API:** Entry lists say who created each entry and can keep one person's (`?created_by=`); a title search looks
  only at titles the role may read (`403 titles_not_readable` when it reads none).

## E · Editorial parity

### Schedules · Publishing at a time you choose

- **Merged:** 2026-10-27 · `bf0ecd7`
- **Decisions:** D-060
- **Proof:** `crates/colophon-domain/src/schedule.rs` (New York's and London's clock changes), `crates/colophon-db/tests/schedules.rs`, `crates/colophon-server/tests/schedules.rs`, `apps/studio/tests/server/schedules.spec.ts`, `apps/testbed/tests/schedules.spec.ts`; `perf/k6/schedules.js` on `db542c4`: [1,000,262 requests at 1,856/s, p99 48.4 ms](perf/results/2026-10-28T032153Z-schedules-throughput-apple-m2-pro-12c.json) and [steady at 1,000/s, p99 10.5 ms](perf/results/2026-10-28T032306Z-schedules-steady-apple-m2-pro-12c.json), nothing failed

Built ahead of the rest of milestone E, on the maintainer's request; the spec's "Scheduled publishing" section and
case X07 set its rules. E's other work (localization, review stages, comments, live collaboration) is planned.

#### Added

- **Studio:** You can schedule an approved publish request to go live at a date and time in a time zone you pick
  (yours first), and cancel it until then. A time the clocks show twice or skip, the night daylight saving starts or
  ends, is never guessed: the Studio shows both moments and schedules the one you pick. A pending schedule says when it
  publishes, there and where you are, and who scheduled it; a past one says whether it went out on time, late, not at
  all and why, or was cancelled. The Releases page lists what is scheduled.
- **Worker:** A schedule publishes once, at its time, as the person who scheduled it, with their role as it is then.
  If the approval expired, the request was edited after approval, or that person can no longer publish it, the
  schedule is blocked with the reason in plain words and nothing is published. A start more than 60 seconds late is
  shown as delayed. Technical: a loop beside the deliveries; the release and the schedule's outcome commit together;
  publishing moved into `colophon_db::publish`, so the API and schedules run one routine.
- **API:** Technical side of the above: `POST …/changesets/{changeset}/schedule` with `local_time`, `time_zone` and an
  optional `choice`, under `If-Match`; `GET …/schedules` and `DELETE …/schedules/{schedule}`; 422
  `ambiguous_local_time` and `skipped_local_time` name both candidate instants; a time must be in the future, within a
  year and a day, and before the approval expires.
- **Database:** Migration 0019 (`schedules`), one pending schedule per publish request.
- **Tests:** The testbed's Schedules page, and `perf/k6/schedules.js`, run as a smoke by `scripts/check.sh` and CI
  (at 500 requests a second in CI, half of what its slowest runner manages). Its first smoke missed its 200 ms p99
  because a list sorted every schedule ever made; lists now read pending and recently finished schedules through their
  own indexes (list p99 586 ms before, 46 ms after, on a busy laptop). Its first full runs, above, are the scenario's
  baselines on this laptop.

## R · Roles and permissions

### R.3 · Permissions down to a field, and to your own entries

- **Merged:** 2026-10-27 · `9991ec2`, `1b4396e`
- **Decisions:** D-058
- **Proof:** `crates/colophon-domain/src/policy.rs` (narrowed permissions, `Policy::fields`, `covers`), `crates/colophon-api/src/visibility.rs`, `crates/colophon-server/tests/permissions.rs` (an author role against the real binary), `apps/studio/tests/server/permissions.spec.ts`; `perf/k6/roles.js` on `db542c4`: [1,000,078 requests at 1,722/s, p99 61.9 ms](perf/results/2026-10-28T031138Z-roles-throughput-apple-m2-pro-12c.json) and [steady at 1,000/s, p99 15.0 ms](perf/results/2026-10-28T031248Z-roles-steady-apple-m2-pro-12c.json), nothing failed

R.4, API tokens and the audit log, is next; milestone R's proof gate isn't met until it is.

#### Added

- **Studio:** Owners can limit a permission to some fields ("Edit: titles only") or to a member's own entries ("Edit:
  Articles, only their own entries"). A role's page lists such limits in words, keeps them when the grid is saved,
  and adds or removes them with a small form. In the editor, fields your role can't see are left out and never sent,
  so saving can't erase what you never saw; fields you can see but not change are shown disabled, with a note saying
  which.
- **API:** Technical side of the above: a permission takes `"fields": [...]` on `content.read`, `content.create` and
  `content.update`, and `"condition": "own"` (entries the member created) on `content.read`, `content.update` and
  `content.delete`; anything else is 422 (`fields_not_applicable`, `invalid_fields`, `condition_not_applicable`,
  `unknown_condition`). Entries leave out the fields a role can't read (`hidden_fields`) and mark those it can read but
  not change (`locked_fields`); an entry it can't read at all is 404; a list holds only the caller's own entries when
  that is all the role may read. Saving refuses a change to a field the role can't change (422 `field_not_permitted`,
  pointing at the field), and puts back every field the saver can't read exactly as stored. A publish request may
  include an entry only if the role may change it.
- **Database:** Migration 0021: a role holds one permission per action, subject and condition, so "titles of any
  article" and "every field of their own" can sit side by side; the database refuses field lists and conditions on
  actions they don't apply to.
- **Tests:** On the real binary, an author role that may change the title of any article and every field of its own
  sees only the owner's title, retitles it while the body it never saw stays as stored, is refused a body it sends
  anyway, and can't put the owner's article in a publish request when narrowed to its own. In the Studio, the same
  author retitles an article without seeing its body, and a role's limits survive a save and a reload.
  `perf/k6/roles.js` doesn't use field lists or `own`: its runs above measure R.3's code with a custom role that has
  neither.

#### Fixed

- **Database:** Owners hold every permission (D-058), but migration 0017 seeded them without `content.delete`, so no
  role could ever be given it. Migration 0022 adds it to every owner role, new and existing. No route deletes entries
  yet.
- **Tests:** CI's roles smoke failed on its slower runners (AMD EPYC 7763) and passed on faster ones (EPYC 9V45),
  with the same code. It now runs at 450 requests a second, about half what a 7763 manages, as CI's other
  database-backed smokes already did; the thresholds are unchanged.

### R.2 · Roles you define

- **Merged:** 2026-10-27 · `c89c102`
- **Decisions:** D-058
- **Proof:** `crates/colophon-server/tests/permissions.rs`, `apps/studio/tests/server/permissions.spec.ts` (passed 4 times in a row), `apps/testbed/tests/roles.spec.ts`; `perf/k6/roles.js` smoke runs; a three-run A/B of `perf/k6/members.js` against main on one laptop (commit `5cc99e4`)

#### Added

- **Studio:** Owners can make roles of their own in Settings, and say what each may do as a grid in plain words (See,
  Create, Edit, Approve, Publish, Change fields), for every content type or one at a time, plus the powers over the
  whole project. Any member can read what a role means before being given it; a role in use can't be deleted, and the
  page says who holds it. Saving names the version you edited, so a second tab can't overwrite your change unseen.
- **Studio:** The Studio offers only what your role allows, from your role's permissions rather than its name;
  members and invitations offer the project's real roles. The server still decides every request.
- **API:** Nobody hands out more than they hold: creating, changing or deleting a role needs every power of that role,
  and changing someone's role, removing them or inviting them needs every power of the roles involved. The owner role
  is fixed. Technical: `GET`/`POST /api/v1/projects/{project}/roles`, `GET`/`PUT`/`DELETE …/roles/{role}` with
  `If-Match: "v<version>"`; 409 `stale_role`, `role_in_use`; at most 100 roles a project; `Policy::covers`.
  `GET /api/v1/projects/{project}` returns your role's name and permissions. On its branch, field lists and conditions
  were refused until R.3, merged the same day, enforced them.
- **Database:** Migration 0018 gives roles a version and makes the owner role refuse any change, even a direct write
  as the application's login; a pending invitation holds its role with an advisory lock instead of a foreign key.
- **Tests:** The testbed's Roles page drives roles through the real API; `perf/k6/roles.js` is a load scenario for
  custom roles, run as a smoke by `scripts/check.sh` and CI. Its first million-request runs were made with R.3's code
  (see R.3).

#### Fixed

- **API:** Found before merging: the first version made member changes slower under load, because every change to one
  project's members waits for the same lock and the new check spent about nine round trips holding it (CI's members
  smoke missed its p99, 232 ms against 200). A change now locks everyone involved in one statement and reads their
  roles in one more. On one laptop, uncontended, a role change takes 4.2 ms against main's 4.0; under
  `perf/k6/members.js`, role-change medians were 9 and 23 ms against 27 to 47 ms on main. Role edits lock only the
  role they change, after the first `roles.js` smoke missed its 200 ms p99 (209 ms).

### R.1 · Roles and permissions as data

- **Merged:** 2026-10-27 · `85aa1f1`
- **Decisions:** D-058
- **Proof:** the D-042 table test for every role in every collection; `crates/colophon-db/tests/roles.rs` holds the seeded roles equal to `Policy::builtin`; the server suites pass unchanged; session load results, [quiet](perf/results/2026-10-27T155703Z-session-steady-apple-m2-pro-12c.json) and [busy, missed](perf/results/2026-10-27T155156Z-session-steady-apple-m2-pro-12c.json)

Nobody sees a difference: what each of the four roles may do is unchanged.

#### Added

- **Tests:** Session load results, both kept: at 2,000 requests a second for 60 seconds on a quiet laptop, p99 12.2
  ms with nothing dropped (main measured the same way: p99 16.2 ms); a first run while the machine was busy with
  other tests missed its threshold (p99 64.6 ms, 59 dropped). Loading the role's permissions costs about 0.1 ms a
  request.

#### Changed

- **Database:** A project's roles and what they may do are now stored as data instead of written into the code, which
  is what lets owners define their own in R.2. Every project, new and existing, starts with the four roles it had,
  with exactly the same powers. Technical: migration 0017 adds `roles` and `role_permissions`, seeds the four roles
  through `colophon_seed_roles`, and makes memberships and invitations name a role of their project by key; one
  deny-by-default evaluator, `colophon_domain::policy`, decides per collection.

## P3 · Prove it reached the site

### 3.7 · The non-technical path

- **Merged:** 2026-10-27 · `76cddf8`
- **Decisions:** D-059, D-061
- **Proof:** `apps/studio/tests/server/journey.spec.ts` (sign-up to a first release on a new installation, axe in light and dark on every screen), `apps/studio/tests/server/words.spec.ts`, `crates/colophon-db/tests/setup.rs`, `crates/colophon-server/tests/setup.rs`, `crates/colophon-server/tests/sign_in.rs`, `apps/testbed/tests/setup.spec.ts`; `perf/k6/setup.js`: [1,000,012 requests at 3,099/s, p99 36.2 ms](perf/results/2026-10-27T211759Z-setup-throughput-apple-m2-pro-12c.json) and [steady at 2,000/s, p99 58.1 ms](perf/results/2026-10-27T210433Z-setup-steady-apple-m2-pro-12c.json), with two first runs on an oversubscribed laptop kept, one of them missed

Done but for one check: D-059's rule 7, every screen readable and reviewable on a phone. So far the journey test
checks two screens at 390 pixels wide (the publish request list and one request) for sideways scrolling, and no
others.

#### Added

- **Studio:** Someone new can go from signing up to a first release without help. Signed out, the Studio offers *Sign
  in* and *Create an account* (the identity provider's own sign-up form). On a new installation, the first person to
  sign in enters the setup code the server printed to its terminal and becomes the administrator; an administrator
  with no project is offered their first; anyone else learns that projects are joined by invitation, and what to ask
  for. The invitation page offers an account too.
- **Studio:** A new project can start as a small blog (Articles with a URL name), a documentation site (Pages with a
  title, a URL name and a body) or blank. Technical: `lib/starters.ts`, applied through the content-type API as the
  new owner; if that fails, the project exists and a message says how to finish it in Settings.
- **Studio:** A checklist on a project's overview: write the first article, publish it (or ask for review), connect
  the website, invite a teammate, see the first receipt. Each item is worked out from what really exists, and items
  your role can't do are left out. Hidden, it stays hidden in that browser.
- **Studio:** *Publish* on an entry: one click makes a publish request for the saved version and sends it for review;
  when the project lets people approve their own requests and your role may approve and publish, the same click
  approves it (recorded as your review) and publishes it. Otherwise the button is *Ask for review*. If another
  release went live first, it stops and says so.
- **Studio:** Empty pages say what belongs there and offer the action that fills them.
- **API:** A fresh installation can be set up without the database's credentials: while nobody is an administrator,
  the server prints a one-time setup code to its terminal when it starts, and the person who enters it, signed in,
  becomes the first administrator. Technical: `POST /api/v1/setup`; the code is 20 characters, valid for an hour,
  single-use, stored only as a SHA-256 hash and compared in constant time; five attempts a person and twenty in all
  per fifteen minutes (429 `rate_limited` with `Retry-After`); every path refuses once an administrator exists (409
  `already_set_up`); migration 0020. Amends D-043; `colophon admin bootstrap` still works.
- **API:** You can create an account from the sign-in page when the identity provider offers sign-up, and `/me` says
  whether the installation has an administrator yet. Technical: `/auth/login?start=create` (`prompt=create`). A new
  account grants nothing by itself.
- **API:** `GET /api/v1/projects/{project}` says whether people may approve their own publish requests
  (`self_review`), which one-click publishing follows. It is always true until projects have settings (D-044).
- **Tests:** The journey on a new installation with its own database and server; a test that fails on an internal
  word in visible text outside Settings and the Technical details drawer; database tests for every property of the
  setup code; real-binary tests on a database nobody set up, and an account made on Keycloak's sign-up form; the
  testbed's Setup page; `perf/k6/setup.js`, run by `scripts/load.sh setup` and not among CI's smokes.
  `scripts/first-run.sh` starts a new installation by hand on port 58400.

#### Changed

- **Studio:** Plain words on every screen, from one glossary (`lib/words.ts`): publish request, version, URL name,
  website connections (rebuild the site, clear the CDN, check the site) and releases by number. Ids, hashes and API
  addresses moved into *Technical details* drawers.
- **Studio:** A new entry's URL name is made from its title, and when another entry has it, the next free one
  (`hello-readers-2`); an edit that collides says which entry has the name.

#### Fixed

- **Studio:** A published request's comparison, opened directly, could say "Loading the comparison…" for good. Found
  by the journey's phone step, which failed every time without the fix.

### 3.6 · Content types and URL names

- **Merged:** 2026-10-27 · `6de850c`
- **Decisions:** D-054, D-055; the branch also carried D-057, D-058 and D-059
- **Proof:** `crates/colophon-db/tests/slugs.rs`, `crates/colophon-server/tests/content_types.rs`, `apps/studio/tests/server/types.spec.ts`; `scripts/check.sh` on a fresh database at `ef5939c`

On its branch, `scripts/check.sh` passed on a fresh database (`ef5939c`: 264 Rust tests, 77 Studio tests, 23 against
the real server, 23 testbed tests). No million-request load result is committed for it.

#### Added

- **Studio:** Owners and developers can build content types in Settings: add fields, set what each accepts, and
  give a type a URL name field. Editors get a URL name suggested from the title in one click. Technical: the
  content-type builder (`TypeBuilder.svelte`) at Settings, then the type.
- **API:** Two entries of one collection can't share a URL name, among drafts or among what is published; a taken one
  is refused naming the entry that has it. A URL name is 1 to 120 lowercase letters and digits joined by single
  hyphens, refused rather than rewritten when it isn't one. Technical: the `slug` field type, 422 `invalid_slug`,
  409 `slug_taken`; migration 0016 adds two partial unique indexes so PostgreSQL itself refuses a duplicate (D-054).
- **API:** Changing a content type is checked against what is stored: removing a field, making one required or
  narrowing a limit is allowed when every draft and every published version still fits, and refused otherwise,
  naming the first 20 entries in the way. Technical: 422 `needs_migration` with the entries and the total,
  `too_many_to_check` above 10,000; writes and publishes take a shared advisory lock per collection and a change
  takes it exclusively (D-055, amending D-040).
- **Delivery:** The reference site serves an article at `/articles/<URL name>/` when it has one, and a build that
  would put two articles at one address fails and leaves the live site alone.
- **Docs:** The scope decisions: every Strapi and Sanity feature at Colophon's bar of proof (D-057), roles and
  permissions as data (D-058), non-technical editors first (D-059), and `docs/COMPARISON.md`; the roadmap gains
  milestones R, M, E and D.

### Fix 3.5 · Title search without walking the whole collection

- **Merged:** 2026-10-27 · `065e614`
- **Proof:** `crates/colophon-db/tests/entries.rs` checks the entry's copy against its draft and that search follows a renamed draft; CI passed on `065e614` (GitHub Actions run 40533914852) with its drafts load smoke still at 400 requests a second, after failing on `d1c9bd7` (run 40526045207)

#### Fixed

- **API:** Searching titles no longer slows down everyone else's requests as a collection grows. Step 3.5's search
  looked up every entry's draft before filtering: in CI's drafts load smoke (512 entries) one search made 512 lookups
  and took 157 ms, and the requests queued behind it reached a p99 of 1.47 s at 400 requests a second. Technical:
  migration 0015 copies each draft's title onto its entry (`draft_title`, backfilled, written with the draft
  pointer) with a `pg_trgm` index, and search filters before it looks anything up. Committed straight to `main`, not
  through a merge; lowering CI's rate instead would have hidden the problem.

### 3.5 · The Studio, designed

- **Merged:** 2026-10-27 · `d1c9bd7`
- **Decisions:** D-053, D-056
- **Build log:** <https://404human.dev/colophon/checkpoint-3-5/>
- **Proof:** `packages/ui/test/tokens.test.ts` (124 contrast checks); axe in the Studio's server suites; entries load results in `perf/results/` from 2026-10-26T211918Z to 2026-10-27T140343Z, `apple-m2-pro-12c`; CI on the merge failed its drafts load smoke (run 40526045207), fixed by `065e614`

#### Added

- **Studio:** You can search a collection by title, see whether each entry is a draft, live, or changed since it went
  live, compare any two versions of an entry, and see exactly what a publish request changes against what is live
  before you approve it.
- **API:** Technical side of the above: entry lists take `?q=` (at most 100 characters, case-insensitive, `%` and `_`
  taken literally) and name the live version and its release; `GET …/entries/{entry}/revisions/{revision}` returns
  one version with its content; publish request items name the version they replace.
- **Studio:** Every screen rebuilt on one design system: quiet paper, black ink and one warm accent, Inter only and
  self-hosted, a sidebar with your projects, collections and the publish requests waiting for review, a command
  palette that searches titles, and a formatting toolbar in the editor. Website connections move to Settings.
  Technical: all 64 shadcn-svelte components owned in `packages/ui` on Colophon's tokens (D-053, D-056);
  `/studio/design` shows every token and component.
- **Studio:** The local sign-in page matches the Studio. Technical: a Keycloak login theme in `deploy/keycloak-theme`,
  styles only, so field ids and flows are unchanged.
- **Studio:** Every text colour is checked against its background: 124 checks pass WCAG AA, and AAA under the
  system's "more contrast" setting. Motion is off under reduced motion.
- **Tests:** The testbed searches titles and reads any version back; the entries load scenario adds searches and
  version reads, and names every request by its route, after k6 itself grew to 1.7 GB tracking one series per
  version address.
- **Tests:** Two entries load runs that missed the regression gate are kept (p99 76.5 and 72.0 ms against a 67.2 ms
  gate, on a busy laptop), and so is a finding: on a local database grown to 5 GB, saves ran at 1,149 a second with a
  p99 of 183.8 ms, against 1,785 a second and 52.4 ms on a fresh one. The growth of the idempotency records is a
  product finding, not only a test artefact.
- **Operations:** `scripts/check.sh` runs its integration checks on a fresh database, as CI does (`colophon_check`,
  unless `COLOPHON_TEST_DATABASE` names another).

#### Changed

- **Studio:** The Material 3 look from step 0.7 is gone: D-053 replaced it (superseding D-011, D-024 and D-025), and
  D-056 replaced D-053's look while keeping its stack, Tailwind CSS and shadcn-svelte.

#### Fixed

- **Studio:** Found by the Studio's tests and fixed before merging: the sidebar is a navigation landmark, the project
  switcher's name includes its visible text, and a closed command palette leaves nothing in the page.

### 3.4 · My own site, running on Colophon

- **Merged:** 2026-10-26 · `0864e22`, `0550292`
- **Decisions:** D-052
- **Build log:** <https://404human.dev/colophon/checkpoint-3-4/>
- **Proof:** `apps/web` tests (pinned reads, escaping, X08, racing promotions); `crates/colophon-server/tests/site.rs`; `perf/results/2026-10-26T193403Z-site-throughput-apple-m2-pro-12c.json`

The site runs locally only; putting it on the internet is a later step of P3 (O-012).

#### Added

- **Delivery:** A website built from Colophon shows exactly one release, never a mix, and every page says which
  release it came from. Technical: `apps/web` (Node 24, TypeScript run from source) reads one release from
  `/content/v1` with every read pinned to it, renders with the same renderer as the Studio's preview, and writes
  `<meta name="colophon-generation">` once on every page.
- **Delivery:** A new version of the site goes live all at once or not at all, and an older release can never
  replace a newer one; a failed build leaves the live site alone. Technical: a build is checked, then promoted with
  one atomic rename under a lock (X08).
- **Delivery:** The site rebuilds itself when a release is announced. Technical: a runner receives each release's
  webhook and builds the newest one at a time; `scripts/demo.sh` runs the CMS, the Studio and the site locally.
- **Tests:** The site under release churn: 1,000,195 page reads at 19,013 a second (p99 6.9 ms) while 27 releases
  were published underneath the readers, every page read whole with one release marker, on one laptop. The 4 failed
  requests are the setup's polls before the first build went live; no read failed.

#### Fixed

- **Studio:** Found while walking through the demo: the header still said "step 1.7", and a receipt printed
  "HTTP 503 (HTTP 503)" when the error was only the status.

### 3.3 · A receipt for every publish

- **Merged:** 2026-10-26 · `c5980d6`
- **Decisions:** D-051
- **Build log:** <https://404human.dev/colophon/checkpoint-3-3/>
- **Proof:** probe and label tests in `colophon-domain`; a release verified at 2 locations in `crates/colophon-server/tests/receipts.rs`; `perf/results/2026-10-26T185935Z-publishing-throughput-apple-m2-pro-12c.json`, and the two failed runs kept beside it

#### Added

- **Studio:** Every release tells you whether it reached the website, in one of four labels: *Published in CMS*,
  *Website update pending*, *Verified at N locations*, or *Needs attention*. "Verified" always says at how many
  locations. Its receipt shows each website connection's part and every check made, oldest first.
- **Worker:** A website connection can check the site: it fetches an address and looks for the release's marker,
  keeping every look (what it looked for, from where, what it saw, the body's SHA-256). Technical: probe
  destinations (migration 0014), retried no sooner than every 5 seconds up to 10 attempts, optionally pinned to a
  location (`COLOPHON_WORKER_LOCATION`, default `local`); `GET …/releases/{generation}` is the receipt.
- **Tests:** Labels and receipts in the testbed, and in the publishing load scenario, whose teardown requires every
  listed release verified at one location: 1,000,393 requests at 941 a second, p99 201 ms, none failed, on one
  laptop.

#### Fixed

- **API:** Found before merging: listing a project's releases, with this step's rule for when a website connection
  is done, timed out at 5 seconds with 3,139 releases; it now reads each connection's latest jobs once and answers in
  6.7 ms on the same data. Two million-request publishing runs had failed on that timeout in their teardown; both
  are kept.
- **API:** Entry lists could scan every version when PostgreSQL's statistics were stale (CI's drafts smoke on `main`
  failed at a p99 of 924 ms, run 40365495381); each entry's draft is now looked up on its own.

### 3.2 · Taking it back

- **Merged:** 2026-10-26 · `f15b0d7`
- **Decisions:** D-049, D-050
- **Build log:** <https://404human.dev/colophon/checkpoint-3-2/>
- **Proof:** F07 at the origin with the cache warm, and end to end against a fake CDN (`crates/colophon-server/tests/purges.rs`); `perf/results/2026-10-26T164943Z-publishing-throughput-apple-m2-pro-12c.json`, `…162630Z-delivery…` and `…163022Z-delivery…`

#### Added

- **Studio:** You can take an article off the website. Taking it down goes through review like any other publish
  request, and afterwards the article isn't served at any address, older release addresses included. Publishing it
  again brings it back from the new release only. Technical: an `unpublish` item in a changeset; its release leaves a
  tombstone and revokes every earlier publication (migration 0012); `/content/v1` answers 410 `gone`, never cached.
- **Worker:** A website connection can be a CDN: each release asks it to drop what changed, and a purge that was
  skipped is covered by the next. The Studio says "removal pending at the CDN" until the purge is accepted.
  Technical: purge-by-tag destinations (migration 0013), each purge covering every release since the last successful
  one; the releases list reports each purge as none, pending, accepted or failed.
- **Tests:** Publishing with takedowns and CDN purges: 1,000,328 requests at 912 a second, p99 145 ms, none failed, on
  one laptop. Two delivery runs with takedown reads are kept: the first had a 12.17 s maximum while the laptop was
  swapping; the second, straight after, peaked at 108 ms.

### 3.1 · A cache that can't serve the wrong thing

- **Merged:** 2026-10-26 · `e5c874a`
- **Decisions:** D-048
- **Build log:** <https://404human.dev/colophon/checkpoint-3-1/>
- **Proof:** F06, F08 and L04 against the stack; `perf/results/2026-10-26T140544Z-delivery-throughput-apple-m2-pro-12c.json`

#### Added

- **Delivery:** Public reads are faster, and a cached answer can never bring back an older release: a new release is
  served at once, and losing the cache loses nothing. Technical: answers cached in Valkey under keys that name their
  release generation; the generation is always read from PostgreSQL first, in one statement (migration 0011).
- **Delivery:** Many readers asking for the same thing at once share one fill of the cache, and when the cache is
  down, reads go to PostgreSQL instead. Technical: single-flight fills per process; a 100 ms deadline on Valkey calls;
  strong ETags with 304; `Cache-Control` (shared caches 30 s for latest, 300 s for pinned), `Cache-Tag` and
  `Colophon-Cache`.
- **Tests:** 1,000,058 requests at 6,340 a second with a p99 of 25 ms, 99.99% of answers without a database read of
  their own, on one laptop; step 2.2's run without the cache did 2,650 a second at 31 ms.

### Fix · The Studio reads a project's role from the project

- **Merged:** 2026-10-26 · `0512497`
- **Proof:** a Studio test that empties the project list on its way to the browser and checks the owner's controls are still there; it fails without the fix

#### Fixed

- **Studio:** If you belonged to more than 1,000 projects, the Studio hid your owner, reviewer and developer controls
  in the rest. It read your role from your list of projects, which stops at 1,000; it now asks the project itself
  (`GET /api/v1/projects/{project}`).

## P2 · Publish exactly once

### 2.4 · Killing it on purpose

- **Merged:** 2026-10-26 · `c38ec47`
- **Decisions:** D-047
- **Build log:** <https://404human.dev/colophon/checkpoint-2-4/>
- **Proof:** `colophon-sim --scenario publishing`, 1,000 seeds in CI, every planted bug caught; `crates/colophon-server/tests/crash_loop.rs`, 12 kills per CI run

With this step, P2's proof gate is met.

#### Added

- **Tests:** A simulator of publishing: people publishing on the release they saw, the outbox, workers that crash at
  any step, and a network that loses, duplicates and refuses. Eight planted bugs, each of which it must catch; CI
  sweeps 1,000 seeds. Technical: `colophon-sim --scenario publishing`; a failing seed replays exactly.
- **Tests:** A real-process crash loop kills the server and worker with SIGKILL at random moments, twelve times per CI
  run, and checks that nothing acknowledged was lost, doubled or delivered out of order. Its first full run: 524
  releases acknowledged and checked, 453 announcements received.

#### Fixed

- **Worker:** Two announcements could be in flight to one website connection at once, so a slow network could deliver
  release N after N+1. Now one is in flight per connection (migration 0010). Found by modelling the worker.
- **Worker:** The worker started its lease clock after claiming a job, while the database starts it during the claim;
  it now starts it before, so it can only overestimate how much of the lease is gone.

### 2.3 · Work that survives a crash

- **Merged:** 2026-10-26 · `070415e`
- **Decisions:** D-046
- **Build log:** <https://404human.dev/colophon/checkpoint-2-3/>
- **Proof:** F03 in `crates/colophon-server/tests/faults.rs`, F05 in `crates/colophon-server/tests/worker.rs`; `perf/results/2026-10-26T122940Z-publishing-steady-apple-m2-pro-12c.json`, and the missed runs kept beside it

#### Added

- **Worker:** When something is published, your website is told, even if the server crashes halfway; a release
  that was already overtaken by a newer one is never sent. Technical: a transactional outbox (migration 0009);
  `crates/colophon-worker` (`--role worker|all`) claims jobs under leases and posts with a stable idempotency key;
  F03 (the worker killed after announcing: the retry sends again with the same key) and F05 (a superseded release is
  never sent).
- **Studio:** Website connections in a project's settings, and what was delivered for each release. Technical:
  destinations must be `https` and public addresses, checked when saved and before every attempt, with no
  redirects followed.
- **Worker:** The worker has its own database pool (`COLOPHON_WORKER_DB_CONNECTIONS`, default 4), so deliveries can't
  take the connections editors' requests need. It came from the first publishing load runs, which missed their
  targets (a p99 of 387 ms against 250, and a steady run at 100 publishes a second that fell behind); those results
  are kept, and so is a later million-request run that didn't finish clean (874,279 requests, 82 failed) on a
  database grown to several gigabytes.
- **Tests:** The publishing load scenario with a local receiver: 40 publishes (209 requests) a second with a p99 of
  19 ms, on one laptop.

### 2.2 · One release, one number

- **Merged:** 2026-10-26 · `2831ef2`
- **Decisions:** D-045
- **Build log:** <https://404human.dev/colophon/checkpoint-2-2/>
- **Proof:** two releases on one base can't both commit (`crates/colophon-db/tests/releases.rs` and end to end); `perf/results/2026-10-26T043905Z-delivery-throughput-apple-m2-pro-12c.json`

#### Added

- **Studio:** You can publish an approved publish request, and every release gets the next number. Two people
  publishing at the same moment can't both win: the second is told someone else published first. Technical: one
  transaction takes the next generation only from the base the publisher saw (409 `generation_moved`); migration
  0008; a Releases page.
- **Delivery:** Any past release can be read exactly as it was. Technical: publication history is non-overlapping
  intervals; `/content/v1` reads at the latest or a pinned generation, through a database role that can't read
  drafts (`colophon_delivery`).
- **Tests:** 1,000,058 anonymous reads at 2,650 a second, p99 31 ms, on one laptop.

### 2.1 · Approving the exact change

- **Merged:** 2026-10-26 · `ec7a3c2`
- **Decisions:** D-044
- **Build log:** <https://404human.dev/colophon/checkpoint-2-1/>
- **Proof:** changeset database tests (insert-only reviews, own versions only, isolation); `perf/results/2026-10-26T040643Z-changesets-throughput-apple-m2-pro-12c.json`

#### Added

- **Studio:** You can gather entries into a publish request, submit it, and approve exactly the versions on your
  screen. Edit anything afterwards and the approval no longer counts: it needs approving again. Technical:
  changesets pin exact revisions and their content-type versions; their SHA-256 hash is computed from them; submit,
  refresh and reviews need `If-Match` with the hash (409 `stale_changeset`); reviews are for owners and publishers
  (migration 0007).
- **Tests:** 1,000,267 requests at 1,461 a second, p99 59 ms, on one laptop. The end-to-end test caught an approval
  losing to a request for changes made in the same second; decisions are now ordered by a sequence, not a timestamp.

## P1 · Never lose an edit

### 1.8 · The first owner

- **Merged:** 2026-10-26 · `2652f43`
- **Decisions:** D-043
- **Build log:** <https://404human.dev/colophon/checkpoint-1-8/>
- **Proof:** F01 and F02 against the real binary, F04 in two browsers, a second project denied on every route; `perf/results/2026-10-26T032419Z-projects-throughput-apple-m2-pro-12c.json`

With this step, P1's proof gate was re-checked and is met.

#### Added

- **CLI:** The person running the server makes the first administrator once, from a terminal; after that, nobody can
  do it again through the web. Only administrators create projects. Technical: `colophon admin bootstrap --subject …`
  binds the administrator to an issuer and subject (migration 0006); a restrictive row-level security
  policy backs the API's check; `/me` and `colophon whoami` show whether you are one.
- **Tests:** 1,000,013 project requests at 3,069 a second, p99 31 ms, on one laptop.

### 1.7 · Inviting people

- **Merged:** 2026-10-25 · `76aac72`
- **Decisions:** D-042
- **Build log:** <https://404human.dev/colophon/checkpoint-1-7/>
- **Proof:** members database tests (the last owner, two owners demoting each other at once, single-use links); `perf/results/2026-10-26T024252Z-members-throughput-apple-m2-pro-12c.json`, and the missed runs kept beside it

#### Added

- **Studio:** Owners invite people with a link that works once, for one role, for 72 hours, and can revoke it; a
  project can never lose its last owner. Technical: owners, developers, editors and publishers, decided by one
  deny-by-default policy table (`colophon_domain::policy`); the token travels after `#`, which browsers never send
  to a server, and is stored only as a hash (migration 0005).
- **Tests:** 1,000,015 requests at 1,993 a second, p99 60 ms, on one laptop. The first runs, which missed their p99,
  are kept.

#### Fixed

- **API:** Found and fixed before merging: an owner demoted in the middle of a request could still manage members.

### 1.6 · The command line saves the same draft

- **Merged:** 2026-10-25 · `6a7157b`
- **Decisions:** D-041
- **Build log:** <https://404human.dev/colophon/checkpoint-1-6/>
- **Proof:** `crates/colophon-server/tests/cli.rs` against the real binaries and Keycloak; `perf/results/2026-10-26T014722Z-cli-throughput-apple-m2-pro-12c.json`

#### Added

- **CLI:** You can sign in from a terminal and create and list entries, the same records the Studio makes. Technical:
  `colophon login`, `logout`, `whoami`, `content create` and `content list`; `--json` prints one versioned result;
  exit codes 2 (usage or invalid content), 3 (denied), 4 (conflict), 5 (unavailable) and 6 (internal).
- **Studio:** You approve a terminal's sign-in with a short code in the Studio (`/studio/device`). Technical: a device
  sign-in after RFC 8628, approved through the browser session (migration 0004); the CLI gets a personal bearer token
  for 30 days, which works on every route and can't approve another device.
- **Tests:** 1,000,009 bearer-token requests at 1,979 a second, p99 59 ms, on one laptop; a steady run at 2,000 a
  second missed and is kept, and one at 1,000 a second passed.

### 1.5 · Content types you define

- **Merged:** 2026-10-25 · `2254117`
- **Decisions:** D-040
- **Build log:** <https://404human.dev/colophon/checkpoint-1-5/>
- **Proof:** content type database tests (versions only from the current one, none rewritten or skipped); `perf/results/2026-10-26T011159Z-content-types-throughput-apple-m2-pro-12c.json`

#### Added

- **Studio:** The editor shows the fields of each content type: text, rich text, number, checkbox, choice and
  date-time, each with its label. Technical: content types are versioned definitions in PostgreSQL (migration 0003);
  each saved version of an entry records which version of its content type it was checked against.
- **API:** A change to a content type is allowed only if every stored entry stays valid. Technical:
  `…/content-types` and `…/content-types/{slug}`.
- **Tests:** 1,000,071 requests at 1,689 a second, p99 60 ms, on one laptop.

#### Changed

- **Format:** `articles` is an ordinary content type now, with the same stored content as before (supersedes D-034's
  single built-in collection).
- **API:** An article title that breaks the rules is now refused with `invalid_text`, the text field's code, instead
  of `invalid_title`.

### 1.4 · The first simulator run

- **Merged:** 2026-10-25 · `542d0eb`
- **Decisions:** D-039
- **Build log:** <https://404human.dev/colophon/checkpoint-1-4/>
- **Proof:** 2,000 seeds of 400 steps in CI; four planted bugs, each caught; the differential test against PostgreSQL

#### Added

- **Tests:** A simulator of colliding editors, lost answers and crashes, which replays any failing run exactly from
  its seed. Technical: `crates/colophon-sim`, seeded, single-threaded and dependency-free; a differential test
  replays simulated runs against PostgreSQL and requires the same answers, save for save.

#### Fixed

- **Studio:** If a save's answer was lost and the page reloaded before the retry, the Studio could store the edit a
  second time, quietly undoing someone else's change. It now resends the unanswered save with its key, so the
  server replays it. Found by the simulator.

### 1.3 · Two editors, one article

- **Merged:** 2026-10-25 · `41733b0`
- **Decisions:** D-037, D-038
- **Build log:** <https://404human.dev/colophon/checkpoint-1-3/>
- **Proof:** F04 end to end in two browsers (`apps/studio/tests/server/`); `perf/results/2026-10-25T234215Z-studio-throughput-apple-m2-pro-12c.json`

#### Added

- **Studio:** The Studio saves as you type, keeps a copy of unsaved edits on your device (for up to 30 days), and
  when someone else saved first, shows both versions side by side instead of overwriting anyone (F04). Every request
  gives up after 15 seconds and is retried with the same key.
- **Operations:** The server serves the Studio at `/studio`, on its own origin. Technical: `COLOPHON_STUDIO_DIR`.
- **Tests:** The Studio's files: 1,000,001 requests at 17,214 a second, p99 8.4 ms, on one laptop; a steady run at
  10,000 a second missed its dropped-starts threshold and is kept.

### 1.2 · A draft that can't be lost

- **Merged:** 2026-10-25 · `00cfba4`
- **Decisions:** D-034, D-035, D-036
- **Build log:** <https://404human.dev/colophon/checkpoint-1-2/>
- **Proof:** F01 and F02 in `crates/colophon-server/tests/faults.rs`; `perf/results/2026-10-25T231601Z-entries-throughput-apple-m2-pro-12c.json`

#### Added

- **API:** A save that was acknowledged is never lost, and a save sent twice is stored once. A save made on an older
  version is refused instead of overwriting newer work. Technical: entries with insert-only revisions (migration
  0002); a save names the revision it was made on with `If-Match` (409 `stale_revision`, 428 without it); writes take
  an `Idempotency-Key` and a retry gets the recorded answer with `Idempotent-Replayed: true`.
- **Tests:** Crash tests F01 (killed before commit: no false acknowledgement) and F02 (killed after commit: the retry
  gets the same answer), through fault points compiled into a test build. Load: 1,000,070 requests at 1,893 a
  second, p99 44 ms, every read returning its writer's last accepted save, on one laptop.

### 1.1 · Signing in, and projects that can't see each other

- **Merged:** 2026-10-25 · `397cb59`
- **Decisions:** D-031, D-032, D-033
- **Build log:** <https://404human.dev/colophon/checkpoint-1-1/>
- **Proof:** row-level security tests against a fresh database; every ID-token attack tested with keys made per run; `perf/results/2026-10-25T222754Z-session-throughput-apple-m2-pro-12c.json`

#### Added

- **API:** You sign in through your organisation's identity provider, and a project you aren't a member of looks
  exactly like one that doesn't exist. Technical: OpenID Connect (Keycloak locally) with PKCE; server-side sessions
  whose cookie holds only a random token; CSRF tokens and exact `Origin` checks on writes; projects isolated by
  PostgreSQL row-level security (migration 0001).
- **Operations:** When the database is busy, requests are told to retry instead of failing. Technical: a full pool
  answers 503 with `Retry-After`, not an internal error; `colophon admin migrate` applies migrations as the table
  owner.
- **Tests:** 1,000,005 signed-in requests at 2,671 a second, p99 30 ms, on one laptop.

## P0 · Foundation

### 0.7 · Material 3 and the Studio's first screen

- **Merged:** 2026-10-25 · `1455073`
- **Decisions:** D-024 to D-030
- **Build log:** <https://404human.dev/colophon/checkpoint-0-7/>
- **Proof:** 116 token contrast tests; axe in four theme and contrast combinations

With this step, P0 is done.

#### Added

- **Studio:** The Studio's first screen, in light and dark, with an Appearance menu. Technical: `packages/ui` on
  Material 3 tokens and Bits UI components (since superseded by D-053 and D-056, step 3.5).
- **Studio:** 116 contrast checks, and axe found no WCAG 2.1 AA violations in four theme and contrast combinations
  after fixing the two it found (an editing area without a name, a panel the keyboard couldn't reach).
- **Docs:** D-024 to D-030 close every P0 question: the colour seed, the components, SQL checked at compile time,
  the OpenAPI generator, a synchronous domain core, SHA-256 and TanStack Query.

### 0.6 · The renderer

- **Merged:** 2026-10-25 · `c195f4a`
- **Decisions:** D-023
- **Build log:** <https://404human.dev/colophon/checkpoint-0-6/>
- **Proof:** 62 Vitest tests, 14 Playwright tests; `fixtures/rich-text/v1/rendered/`

#### Added

- **Studio:** A Preview tab that shows a document exactly as the public site will. Technical:
  `packages/content-renderer`, one renderer for the Studio's preview and the public site, building every tag itself
  and escaping every piece of text.
- **Format:** The reviewed HTML for every valid fixture became part of the shared contract.

#### Fixed

- **Studio:** Found while testing, before merging: ten invalid fixtures crashed the whole Studio; the preview now
  explains a document it can't render instead.

### 0.5 · The Rust rich-text validator

- **Merged:** 2026-10-25 · `906a19e`
- **Build log:** <https://404human.dev/colophon/checkpoint-0-5/>
- **Proof:** all 12 valid fixtures accepted byte for byte, all 34 invalid ones refused with exactly their code and pointer; `perf/results/2026-10-25T204420Z-rich-text-throughput-apple-m2-pro-12c.json`

#### Added

- **API:** The server is the authority on rich text: it stores exactly one form of a document and refuses anything
  else with a code and the place it went wrong. Technical: `colophon-domain` validates in one pass and writes the
  canonical bytes; refusals carry a stable code and a JSON Pointer; `POST /api/v1/rich-text/validate`.
- **Format:** Links may not contain control characters, whitespace or backslashes, which browsers would read as
  another address; four new invalid fixtures hold it.
- **Tests:** 1,000,000 validations at 71,270 a second, p99 2.28 ms, on one laptop.

### 0.4 · The rich-text format, shared fixtures and the editor spike

- **Merged:** 2026-10-25 · `eb742b4`
- **Decisions:** D-021, D-022
- **Build log:** <https://404human.dev/colophon/checkpoint-0-4/>
- **Proof:** 54 Vitest and 50 Playwright tests over the shared fixtures

#### Added

- **Format:** `colophon/rich-text` v1: Tiptap JSON behind an allowlist, with limits (depth 16, 256 KiB, 10,000
  nodes), a canonical form and error codes. Stored v1 documents stay valid for good.
- **Format:** Shared fixtures of valid and invalid documents, used by every implementation.
- **Studio:** The editor opens a document only if it can give it back byte for byte, so nothing is changed silently
  on save. Technical: `packages/editor` and the Studio's editor spike.

### 0.3 · Errors, honest health, the testbed and load tests

- **Merged:** 2026-10-25 · `8d0f80f`
- **Decisions:** D-018, D-019, D-020
- **Build log:** <https://404human.dev/colophon/checkpoint-0-3/>
- **Proof:** `perf/results/2026-10-25T185934Z-throughput-apple-m2-pro-12c.json`; two steady runs that missed their own threshold kept (`…185523Z-steady…`, `…185648Z-steady…`)

#### Added

- **API:** Every error comes back in one shape, with a stable code and the request's id. Technical: one JSON error
  envelope; the id travels in `x-request-id`.
- **Operations:** The server says honestly whether it can work. Technical: `/livez`, `/readyz` (200 ready or
  degraded, 503 not ready) and `colophon doctor`.
- **Tests:** The testing rule (D-018): every feature gets regression tests, a page in `apps/testbed` that exercises
  the real API, and a load scenario whose results are committed, failures included. Load: 5,000,000 health requests
  at 75,405 a second, p99 1.64 ms, on one laptop.

#### Fixed

- **API:** Found by the testbed before merging: when a handler answered without reading the request body, the server
  closed the connection without saying so, and pooled clients failed their next request. The body is now drained, or
  the answer says `Connection: close`.

### 0.2 · The local stack

- **Merged:** 2026-10-25 · `6994073`
- **Decisions:** D-015, D-016, D-017
- **Build log:** <https://404human.dev/colophon/checkpoint-0-2/>
- **Proof:** `scripts/stack.sh up` runs 28 checks through the published ports

#### Added

- **Operations:** One command starts everything Colophon needs on your machine and checks each part works.
  Technical: `deploy/compose.yaml` runs PostgreSQL 18.6, Valkey 9.0.6, Garage 2.5.0 and Keycloak 26.8.1, pinned by
  tag and digest, every port on `127.0.0.1`; `scripts/stack.sh up`.
- **Operations:** Each service must also refuse what it should: a wrong password, a command before `AUTH`, an
  anonymous read, an upload whose bytes don't match its SHA-256, a sign-in without PKCE, the password grant.

### 0.1 · Pinned toolchain, workspace and CI

- **Merged:** 2026-10-25 · `68f471b`
- **Decisions:** D-013, D-014
- **Build log:** <https://404human.dev/colophon/checkpoint-0-1/>

#### Added

- **Operations:** Every build uses the same compiler, Rust 1.99.1, and a check fails when the compiler on `PATH`
  ignores the pin. A five-crate workspace, and a check that keeps I/O out of the domain crate.
- **Operations:** `scripts/check.sh` runs exactly what CI runs; CI pins every third-party action to a commit.
