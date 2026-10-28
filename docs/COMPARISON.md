# Strapi, Sanity and Colophon

Since D-057 Colophon's goal is every feature Strapi and Sanity offer, built at its own bar of proof. This page records
what each does, how Colophon does or will do it, and where Colophon's invariants make it work differently. The
[parity backlog](#parity-backlog) at the end is the list the lettered milestones in [ROADMAP.md](ROADMAP.md) work
through.

**Evidence, checked 2026-10-27.** Strapi: a local Strapi 5.59.0 project in `references/strapi-demo` (its schema files
and installed packages) and docs.strapi.io. Sanity: sanity.io/docs only; there is no local Sanity project. Pricing and
plan tiers are the vendors' own claims and change often. Where a cell says *not checked*, nobody has looked.

## Content modelling

| | Strapi 5 | Sanity | Colophon | Take |
|---|---|---|---|---|
| Field types | Text (short, long), rich text (Blocks or Markdown), number (integer, big integer, decimal, float), date/datetime/time, boolean, enumeration, email, password, JSON, UID, media, relation (six kinds), component (single or repeatable), dynamic zone, marketplace custom fields | string, text, number, boolean, date, datetime, url, email, slug, image, file, reference, cross-dataset and global references, array, object, block (Portable Text), geopoint | text, rich text, number, boolean, enum, datetime (D-040); image with media (P4); reference and bounded list planned (module 01) | **Slug moves into 3.6** (below). Image, reference and list stay as planned. Password, JSON, geopoint, Markdown rich text: not adopted |
| Groups of fields | Components: reusable, single or repeatable, not content on their own | Object types, nestable, arrays of objects | None | Later, as bounded lists of objects; nothing on the reference site needs them yet |
| Page builder | Dynamic zones: an ordered list of components | Arrays of object types | Rich text has its own validated node set | Not for v0.1. Rich text covers the blog; a block list is a format change with new fixtures ([rich-text v1](formats/rich-text-v1.md)) |
| One-of-a-kind content | Single types (the demo's `global` and `about`) | Documents with a fixed id, arranged in the Structure tool | Collections only | Not scheduled. A collection with one entry works until a real need appears |
| Validation | required, unique, min/max length or value, regex, default value | Rule chains (`required`, `either`, `all`), `custom()` functions in the Studio | required, min/max characters, min/max, integer, patterns (whole value, linear time: D-069), defaults filled on creation (D-071); server-side, with a code per failure | Uniqueness for slugs and any `unique` top-level key field (D-070), enforced by the database (spec §07: prechecks alone race). No custom-code validators: the server runs no user code |
| Conditional fields | Shown or hidden by a boolean or enumeration field | `hidden` and `readOnly` callbacks | None | Not adopted for v0.1 |
| AI schema design | Strapi AI (Growth plan) builds content types from prompts, code, Figma or an image | *Not checked* | Agents propose drafts only (P5) | Not adopted. A schema change is the riskiest edit a CMS has; it stays a reviewed human action |

## Changing a schema that already has content

This is where the products differ most, and where Colophon's builder (step 3.6) has to be visibly better.

| | Strapi 5 | Sanity | Colophon |
|---|---|---|---|
| Where the schema lives | JSON files in the project, written by the Content-Type Builder | JavaScript/TypeScript in the Studio's code | Versioned definitions in PostgreSQL, changed through the API (D-040) |
| Who can change it, where | The builder is "available in Development environment only"; changes reach production as a code deploy | Developers, by deploying Studio code | Owners and developers, at runtime, through the same API as everything else (invariant 5) |
| Does the store enforce it | Yes, through database columns | No. "Changing the schema … will not automatically change or delete existing content" | Yes. Every revision records the definition version it was validated against |
| Renaming a field | Since 5.57.0 the column is renamed and data kept; renaming *and* changing its type recreates the field empty | Old data stays under the old name until a migration script moves it | Refused with `needs_migration` today; migrations with plans and approvals come later |
| Removing a type | Entries vanish from the admin; data stays in the database | Documents stay in the dataset | Not possible yet |

**The gap Colophon fills:** neither tool tells the editor *before* a change which stored entries it would break.
Strapi can recreate a field empty; Sanity leaves stale fields for a script to find. Colophon already refuses any
change that would invalidate a stored revision. Step 3.6 makes that refusal useful: the builder names the entries
and revisions that block the change, so an owner can fix them or keep the old shape.

## Editing, history and review

| | Strapi 5 | Sanity | Colophon |
|---|---|---|---|
| Drafts | Draft & Publish, per content type | One draft per document; later edits update it | Insert-only revisions, saves with `If-Match`, autosave, a device copy and a conflict screen (P1) |
| Live co-editing | *Not checked* | Real-time, with presence | Not a goal (spec §01: no CRDT co-editing). Conflicts are detected and shown, never merged silently |
| History | Content History, paid (Growth, Enterprise): 14 days on Growth, 30 to 90 on Enterprise, and only for edits made in the admin panel. REST, GraphQL, Document Service and import changes are not versioned | *Not checked* | Every revision from every path (Studio, CLI, API) is kept for good; the release slider and pinned pages are P4 |
| Review | Review Workflows (the package is installed in the demo; plan tier *not checked*) | *Not checked* | Changesets pin exact revisions; a review approves one hash; no self-approval (2.1) |

## Publishing and delivery

| | Strapi 5 | Sanity | Colophon |
|---|---|---|---|
| Publishing several entries together | Content Releases (the package is installed in the demo) | Content Releases, on some Enterprise plans: at most 1,000 documents and 100 MB a release; Scheduled Drafts for one document on Growth | A release is one transaction on the publisher's base generation; generations only move forward (2.2) |
| Telling other systems | Webhooks | GROQ-powered webhooks, which per Sanity's docs don't yet support the API version that release versions need | A transactional outbox and a leased worker, a stable idempotency key, public addresses only (2.3) |
| Knowing it reached the site | Not offered | Not offered | Probes look for the release's generation marker; receipts say *verified at N locations* or *pending* (3.3) |
| Taking content back | Unpublish | Unpublish | Tombstones, origin revocation through historical URLs, CDN purge jobs, remaining exposure shown (3.2) |
| Reading drafts and releases | Preview | Perspectives: `drafts`, `published`, or a stack of releases | Reads pinned to a generation (2.2); preview links are v0.1 core |
| Query language | REST, GraphQL plugin | GROQ, GraphQL | REST (`/content/v1`) and a TypeScript SDK; GraphQL is a non-goal |

## Done differently, on purpose

- **Plugins.** Arbitrary untrusted native plugins stay out (D-057): code from strangers running in the server would
  break the invariants every other feature relies on. Milestone D offers the same reach safely: custom field types
  declared as data, webhooks by event, and Studio panels in sandboxed iframes.
- **Schema changes as code deploys.** Content types stay data, versioned in PostgreSQL, so the same authorization,
  history and refusal rules apply to them as to content.
- **Flexibility arrives with its proof.** Dynamic zones, nested objects and conditional fields are in milestone M; each
  ships with the validator, renderer and simulator coverage it needs, not before.

## What the comparison changed in step 3.6

1. **Step 3.6 adds a slug field type** (Sanity's `slug`, Strapi's `uid`): lowercase letters, digits and hyphens (refused, not rewritten, when
   it isn't one; the Studio suggests it), unique per project,
   environment and collection (and locale, once locales exist), with uniqueness enforced by PostgreSQL. The reference
   site moves from `/articles/{entry id}/` to `/articles/{slug}/` before step 3.7 makes its URLs public, because public
   URLs are hard to change later. Redirects for a changed slug stay in P4 (module 15).
2. **Step 3.6's builder shows what blocks a change.** A refused definition names the stored revisions it would
   invalidate, which neither Strapi nor Sanity does.
3. **SEO fields are fields, not a feature.** Description and social title are text fields the builder can add; module
   15's SEO list needs no new type except the slug and, in P4, the image.

## Convex: what Colophon borrows for speed

Convex is not a CMS, but its engine is built for fast, always-consistent reads, and several of its ideas fit
Colophon's invariants. Source: "How Convex Works" (stack.convex.dev) and docs.convex.dev, read 2026-10-27, plus the
self-hosted demo in `references/convex-demo`. What Convex does, what Colophon takes, and where:

| Convex | Colophon | Where |
|---|---|---|
| **Reactive queries.** A query records its read set; one pass over the commit log finds every subscription a write touches; the sync worker reruns it and pushes the result over a WebSocket | Live Studio: lists, drafts, changeset status and receipts update without polling. One listener per server on PostgreSQL's commit stream (`LISTEN/NOTIFY` hints, or logical replication), fanned out to bounded per-session queues; a hint only says *what* changed, and the client re-reads through the API, so PostgreSQL stays the authority (invariant 1) | E (with O-010 and O-017) |
| **A cache that is always consistent.** Cached results are served only while their read set has no newer write | Already the delivery cache's rule (D-048: keys name the generation, read from Postgres each time). Extend it to Studio and draft reads with a per-collection version as the read set | D |
| **One snapshot per client.** All of a client's queries are answered at the same timestamp, so the UI never shows a mix | Studio pages read at one generation and draft version; delivery already pins a generation | E |
| **Optimistic concurrency, retried automatically.** Mutations record reads and writes; a conflict aborts and reruns safely because mutations have no side effects | Kept for machine work (whole-transaction retries, spec §08). **Not** for a person's edit: retrying a save over someone else's change would silently overwrite it, so a stale save stays a visible 409 (D2) | — |
| **Side effects live outside transactions.** Mutations are sandboxed; `fetch` only in actions | Already invariant 2: the outbox and the worker | done (P2) |
| **Deterministic functions.** Required for precise subscriptions and safe retries | Already how the simulator works (D-039) | done (P1) |
| **Scheduler and cron as durable rows** | Scheduled publishing on the leased job table | E |
| **Indexes declared in the schema; queries use them** | The query API filters and sorts only on fields a content type declares as indexed, so no query scans a table; pages stay bounded | D |
| **End-to-end types from the schema** | The TypeScript SDK generates types from content-type definitions | D |
| **Random ids spread writes across shards** | Not for PostgreSQL: Colophon keeps UUIDv7 ids so B-tree inserts stay local. The lesson cuts the other way for idempotency keys, whose random text keys scatter index inserts: on a 5 GB local database (2.4 M records) saves ran at 1,149/s with p99 184 ms, against 1,785/s and 52 ms on a fresh one. Next: a time-ordered key column, or tables partitioned by day so retention drops a partition | R (before load scenarios grow) |

Not taken: running user code inside the database (Convex runs TypeScript in V8; Colophon runs no code from strangers,
D-057) and a custom transaction log (PostgreSQL is the authority).

## Parity backlog

Status: **done** (merged, with its proof), **building** (a step in progress), or the milestone that will build it.
Strapi and Sanity columns say who offers it today. Only the rows also covered by the tables above were checked against
the vendors' docs (2026-10-27); the rest, and every plan tier, are from general knowledge and get checked by the step
that builds them.

| Feature | Strapi | Sanity | Colophon |
|---|---|---|---|
| Drafts with autosave, conflict detection | ✓ | ✓ | done (P1) |
| Full history of every revision, from every write path | Paid, admin edits only | ✓ | done (P1); history UI and restore in P4 |
| Content types defined at runtime, safe changes | Dev-only builder | Code | done (D-040; the builder and changes checked against stored content, 3.6) |
| Text, rich text, number, boolean, enum, date-time | ✓ | ✓ | done (D-040) |
| Slug / UID, unique per collection | ✓ | ✓ | done (3.6, D-054) |
| References / relations (one, many, two-way) | ✓ | ✓ | one and many built in M.2 (D-065), never dangling, with "referenced by" on every read; the Studio's picker next |
| Components / objects, repeatable lists | ✓ | ✓ | built in M.3 (D-066), inline, and shared components in M.4 (D-073): one change reaches every type that uses it, checked against stored content |
| Dynamic zones / block arrays (page builder) | ✓ | ✓ | built in M.3 (D-066) |
| Single types / singletons | ✓ | ✓ | built in M.4 (D-067), one entry enforced by the database |
| Email, URL, date, time, JSON, Markdown, colour, geopoint fields | ✓ | ✓ | built in M.1 (D-064) but geopoint; the Studio's editor for them is next |
| Validation: pattern, unique, length, value; conditional fields; defaults | ✓ | ✓ | length and value built; patterns (D-069), unique fields (D-070), defaults (D-071) and conditional fields (D-072) built in M.4 |
| Field descriptions, groups and tabs, custom previews | ✓ | ✓ | descriptions in M.1 (D-064); groups, tabs and previews in M |
| Schema migrations with a plan | Rename only | Scripts | M |
| Media library: uploads, folders, alt text, image transforms, hotspot and crop | ✓ | ✓ | P4 |
| Review before publish | Review workflows (paid) | Tasks, comments | done: changesets and hash-bound reviews (P2); stages in E |
| Publish several entries together (releases) | ✓ | ✓ (Enterprise) | done (P2) |
| Proof that a release reached the site | – | – | done (P3) |
| Unpublish with revocation | Unpublish | Unpublish | done (P3) |
| Scheduled publishing | ✓ | ✓ | done (D-060) |
| Localization (i18n) | ✓ | Plugins | E (data model ready since D-034) |
| Comments and tasks | – | ✓ | E |
| Real-time collaboration and presence | – | ✓ | E (O-017) |
| Visual preview / click-to-edit | Preview | Presentation | E |
| Bulk actions, duplicate, saved views | ✓ | ✓ | E |
| Roles: built-in and custom | ✓ | ✓ (by plan) | done (R.1, R.2; D-058) |
| Field-level and "own entries" permissions | ✓ | ✓ (by plan) | done (R.3; D-058) |
| API tokens: read-only, full, custom | ✓ | ✓ | built in R.4 (D-063): read-only, full access and custom; device sign-in for the CLI since D-041 |
| Audit log | Paid | ✓ (by plan) | built in R.4 (D-063): roles, members, invitations and tokens, append-only |
| SSO / OpenID Connect | Paid | ✓ (by plan) | done (Keycloak, D-031) |
| REST delivery with caching | ✓ | ✓ | done (P2, P3) |
| Query API: filters, sort, populate, field selection, perspectives | ✓ | GROQ | D |
| GraphQL | Plugin | ✓ | D |
| Webhooks | ✓ | ✓ | done (P2); by event in D |
| TypeScript SDK and generated types | ✓ | ✓ | D |
| Import and export, from Strapi and Sanity | Transfer | CLI | D |
| CLI | ✓ | ✓ | done (P1); full contract in P5 |
| Extension points (custom fields, Studio panels) | Plugins | Plugins | D, sandboxed (D-057) |
| AI assistance | Strapi AI (paid) | AI Assist | P5, agents propose only |
