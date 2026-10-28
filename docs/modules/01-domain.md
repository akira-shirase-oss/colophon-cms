# 01 · domain (`crates/colophon-domain`)

> The rules of Colophon: content model, editorial workflow, releases and every state machine, written as pure Rust
> with no I/O.

| | |
|---|---|
| Milestones | P0 (IDs, schema types, rich-text validator), P1 (entries, revisions), P2 (review, releases, jobs), P3 (delivery states), P4 (rollback, scheduling) |
| Proposed owner | Maintainer, with Claude pairing and reviewing |
| Depends on | Nothing that does I/O: `serde`, `uuid`, `thiserror`, `jiff`, a hash crate (O-006) |
| Used by | api, delivery, worker, media, automation, sim; implemented by db |
| Spec | §06, §07, §08, §14, §16 |

## Why it exists

Correctness has to live in one place that every interface shares. If the Studio, the CLI and agents each enforced
rules, they would drift. Keeping the rules free of I/O also makes them testable with a fake clock and lets the
simulator drive the exact same code that production runs (D-005).

## End goal

Every rule about content, review, publication, delivery and jobs is expressed here as types and transitions, with a
test for every legal and illegal transition and property tests showing that random operation sequences never break an
invariant. Production adapters and the simulator both run this code unchanged.

## Responsibilities

- Typed identifiers and scopes for everything.
- The schema model and content validation.
- Entries, immutable revisions, locales and slugs.
- Changesets, reviews and approvals.
- Releases, generations, epochs and publication history.
- Delivery, job and schedule state machines.
- Idempotency semantics.
- Port traits that adapters implement.
- Domain errors expressed in domain terms.

## Not its job

- SQL, HTTP, async runtimes or network calls.
- Authentication protocols. The permission model lives in [auth](03-auth.md); the domain only receives an
  `Actor` and an authorization decision.
- Serialization wire formats beyond `serde` derives.
- Rendering rich text ([content-renderer](12-content-renderer.md)).

## Breakdown

### Identity and scope
- Newtypes over UUID v7: `OrgId`, `ProjectId`, `EnvironmentId`, `EntryId`, `RevisionId`, `ChangesetId`, `ReleaseId`,
  `JobId`, `AssetId`, `PrincipalId`.
- `Scope { org, project, environment }` travels with every command. A user-supplied ID never establishes authority on
  its own (spec §08).
- `Actor`: a human user or an agent service account, with the request ID used for attribution and audit.
- Built in step 1.7 (D-042): `colophon_domain::policy`, with `Role` (owner, developer, editor, publisher), `Action`
  (read, write content, change content types, manage members) and `allows(role, action)`, the one table every
  handler asks.

### Schema model (spec §07)
- Collections with versioned definitions: field ID, type, validation, references, visibility (public or private), and
  editable roles.
- Field types: text, rich text, number, boolean, datetime, enum, image, reference, bounded list.
- Validation uses a constrained, documented subset of JSON Schema, never user-executed code.
- Unknown fields fail validation. Data is never dropped silently.
- A canonical schema hash identifies each version.
- Built in step 1.5 (D-040): `colophon_domain::schema::ContentType` parses a definition (text, rich text, number,
  boolean, enum, datetime; images, references and lists later), validates content into canonical JSON with a code and
  JSON Pointer for every refusal, writes the definition canonically for hashing, and `check_successor` allows only
  changes that keep stored entries valid. `ContentType::articles()` replaced `entry::Article` with byte-identical
  output.
- Built in step 3.6 (D-054, D-055): a seventh type, `slug` (at most one per content type, with an optional `source`
  text field the Studio suggests it from; 1 to 120 lowercase ASCII letters and digits in groups joined by single
  hyphens, refused rather than normalized); `Content::slug()`. `check_successor` became `successor`, which answers
  `Compatible` (widening: nothing to check), `CheckContent` (narrowing: allowed only if the new definition `accepts`
  every current draft and published revision) or `Refused` (a new title field, or a field turned into or out of a
  slug).
- Built in step M.1 (D-064): seven more types and two options. `email` (a dot-atom local part and a host name with a
  dot; shape, never delivery), `url` (`http`/`https` with a host and no credentials, stored in its WHATWG form, what a
  browser's `new URL(text).href` gives), `date` (`YYYY-MM-DD`), `time` (`HH:MM` or `HH:MM:SS`), `json` (any value up to
  64 KiB and 32 levels, keys sorted by UTF-16 code units as JavaScript sorts them, numbers as number fields write
  them), `markdown` (multi-line text with `max_chars`; the site renders it, nothing renders it here) and `color`
  (`#rrggbb` in lowercase). Every field may carry a `description` (help text, up to 500 characters) and text a
  `min_chars`. Both are written only when set, so a definition from before M.1 keeps its canonical bytes and hash,
  which publish requests pin. Changing a field's type is `CheckContent` (text may become email if every stored value
  is one); text widens into markdown that fits it. Not yet: patterns, uniqueness, defaults and conditional fields
  (M.2), and the Studio's builder and editor for the new types (the next slice).
- Built in step M.2 (D-065): `reference` fields (`to` a collection slug; one entry, or `many` with `max` up to 100)
  hold entry ids as canonical lowercase UUIDs, distinct in a list. `Content::references` reports each with its field,
  collection, place and whether the field is a list; whether an id names an entry is the database's to check.
  Pointing a field at another collection is `Refused`.

### Rich text (spec §05)
- Versioned Tiptap JSON is validated against an allowlist of nodes, marks, link schemes and embeds: the
  `colophon/rich-text` v1 format (D-021, [formats/rich-text-v1.md](../formats/rich-text-v1.md)), with error codes and
  JSON Pointers for every rejection. Built in step 0.5: `colophon_domain::rich_text::validate`, which also writes the
  canonical form that is stored.
- Limits: depth ≤ 16 and a maximum document size.
- No executable HTML or components are ever stored.
- The same JSON fixtures are used by this validator and by the TypeScript renderer.

### Entries and revisions
- `Entry`: a stable ID, collection, locale variants, a current draft pointer per locale, and an archived flag.
- `Revision` is immutable. It holds:
  - ID, schema version and structured data;
  - author (human or agent) and base revision;
  - timestamp and content hash.
- **Save rule:** a save succeeds only if `expected_revision` equals the current draft pointer; otherwise it is a
  conflict carrying the current revision.
- Restoring an old revision creates a new revision.
- Built in step 1.2 (D-034): `colophon_domain::entry::Article`, the content of every revision until content types
  exist. One collection, `articles`; a title (1 to 200 characters, no control characters or surrounding whitespace,
  refused rather than trimmed) and a rich-text body; any other field is refused with its JSON Pointer.
  `Article::canonical_json` is the exact text stored and hashed, escaped as `JSON.stringify` escapes it. The save rule
  (D2) is enforced by the database's conditional update in step 1.2; the simulator's in-memory store applies the same
  rule in step 1.4.

### Slugs and locales
- Slugs are unique per project + environment + collection (+ locale, once locales exist), among drafts and among
  open publications; the database enforces both (D-054, migration 0016). The server refuses a slug that isn't one;
  the Studio normalizes as the editor types.
- One logical entry groups its locale variants. Each variant has its own slug, draft and publication history.
- Locale fallback is an explicit project policy, none by default. A fallback resolves only to published revisions and
  labels the resolved locale.

### Editorial workflow (spec §06)
- Editorial states: `draft → submitted → changes_requested | approved`. Publication is a separate state.
- `Changeset`: the exact entry, revision, schema and asset hashes.
- A review decision binds to the changeset hash. Any edit, policy change or expiry invalidates it.
- Self-review is allowed by default but recorded visibly; a project can require a different reviewer.
- **Agents can never approve their own proposals.**
- Built in step 2.1 (D-044): `colophon_domain::changeset`. `Item` pins an entry's revision with its content hash and
  content-type version; `canonical` is the text whose SHA-256 is the changeset's hash (a version line, then one line
  per item, sorted by entry id); `status` decides `Open`, `Stale`, `AwaitingReview`, `ChangesRequested`,
  `Approved { self_review }` or `ApprovalExpired` from the facts: only decisions on the current hash count, the
  highest sequence number wins, approvals last seven days, and a project may require a different reviewer.

### Releases (spec §14)
- `Release`: `prepared → committed`, identified by `(epoch, generation)`.
- The generation increases by exactly one per committed release per environment.
- **Publication history:** half-open intervals `[valid_from, valid_to)` over generations per entry and locale. They
  never overlap.
- Rollback creates a new generation that references older revisions. Generations are never decremented.
- Built in step 2.2 (D-045): `colophon_domain::release`: `next_generation(base, current)` refuses a moved base;
  `Interval` and `apply` model an entry's history, `well_formed` checks it (ordered, non-empty, non-overlapping, at
  most one open, last); and changesets gain `Status::Published { generation }`.
- Unpublish creates a tombstone and a removal job. Built in step 3.2 (D-049): an `unpublish` changeset item pins the
  published revision; its release closes the publication, leaves a tombstone and revokes earlier publications
  (`colophon_domain::release::{apply_unpublish, deliverable}`).
- A first release batch holds at most 100 changed entries.
- Dependency validation: a public entry may not embed an unpublished or private revision.

### Delivery and job state machines (spec §14, §16)
- Destination delivery: `pending → dispatched → observed`, or `retrying | failed | uncertain | superseded`.
- Built in step 3.3 (D-051): `colophon_domain::receipt` (`probe`, `marker`, `label`). A destination is done for a
  release once a job of its at that release or later succeeded; only probes verify, and only at the locations seen.
- Evidence type: release generation, marker or response hash, observation location, timestamp, errors.
- Job states: `pending | running | retrying | succeeded | failed | uncertain | cancelled | superseded`.
- Retry policy as data: 10 s attempt deadline, ≤ 5 attempts, capped backoff with jitter, 1 h lifetime.
- Lease semantics: owner, expiry and attempt generation. Completion is accepted only from the current lease.
- Built in step 2.3 (D-046): `colophon_domain::job` (statuses, `classify` for HTTP outcomes, `backoff_ms` with full
  jitter, `after_attempt` for the attempt budget and deadline) and `colophon_domain::destination` (`check_url`,
  `is_public`).

### Scheduling (spec §06)
- A schedule stores the chosen local time, the IANA time zone, the resolved UTC instant and the approval hash.
- Ambiguous or nonexistent daylight-saving times require an explicit choice.
- At execution, authority, approval, revision hashes and dependencies are rechecked. Edited content blocks the release
  rather than silently changing it.
- A late execution is labelled *delayed*.
- Built (D-060): `colophon_domain::schedule::resolve(local, zone, choice)` resolves with jiff's bundled tz database
  and refuses a time the clocks show twice (`Ambiguous`) or skip (`Skipped`) with both candidate instants unless a
  choice is given; `check_window` keeps it in the future, within a year and a day, and before the approval expires
  (`APPROVAL_LIFETIME_SECS`); `delayed` is a start more than `DELAY_GRACE_SECS` (60) late. States: scheduled,
  published, blocked, cancelled. Tested on New York's and London's 2026 changes.

### Idempotency
- Key semantics are scope + actor + operation + key + request hash.
- The same request returns the recorded result; a different payload with the same key is a conflict.

### Ports (traits implemented by adapters)
- `Clock`: injected time, so tests and the simulator control it.
- `IdGenerator`.
- The content store as a transactional unit of work.
- An outbox writer, published in the same unit of work.
- `DestinationAdapter`, with the spec §25 contract: `validate_configuration`, `plan`, `execute`, `inspect`, `reconcile`,
  `revoke`.
- `ObjectStore`.

Port rules (rust-mastery chapter 10):
- Express errors in domain terms.
- Keep each port narrow.
- Require `Send + Sync` where ports are shared.
- Never let an adapter type appear in a domain signature.

## Invariants

| ID | Invariant |
|---|---|
| D1 | Revisions are never modified. A pinned revision is never deleted. Restoring creates a new revision. |
| D2 | A save succeeds only when `expected_revision` matches the draft pointer. |
| D3 | Every reference stays within one project and environment. |
| D4 | Within an epoch, the generation strictly increases per environment. A restore starts a new epoch. |
| D5 | Publication intervals for one entry and locale never overlap. |
| D6 | An approval applies only to the exact changeset hash it was given for. |
| D7 | An agent cannot approve its own proposal. Nobody can approve their own permission escalation. |
| D8 | A public projection contains only schema-approved public fields of published revisions. |
| D9 | A job result is accepted only from the current lease holder and attempt generation. |
| D10 | A superseded delivery can never make an older generation current. |
| D11 | Unknown fields and unsupported rich-text nodes are rejected, never dropped. |

## Interfaces

- **Commands:**
  - `SaveDraft`, `SubmitChangeset`, `Review`, `CommitRelease`, `Rollback`, `Unpublish`;
  - `Schedule`, `CancelSchedule`, `RecordDeliveryOutcome`.
- **Queries:**
  - the published view at a generation;
  - publication history;
  - pending approvals.
- **Errors (`thiserror`):**
  - `Validation { fields }`;
  - `Conflict { current }`;
  - `Denied`;
  - `NotFound`;
  - `InvalidTransition`;
  - `Stale { reason }`.

## Failure modes

| Situation | Behaviour |
|---|---|
| Invalid input | `Validation` with field paths; nothing committed |
| Concurrent edit | `Conflict` with the current revision; the caller decides to merge or discard |
| Illegal transition (bug or race) | `InvalidTransition`; logged once with context; never silently ignored |
| Stale approval or schedule | Blocked with a reason; requires revalidation |

## Tests and evidence

- A unit test for every legal and illegal transition, using a manual clock.
- `proptest` sequences of saves, reviews, releases, rollbacks and crashes that preserve D1–D11.
- `insta` snapshots of validation error shapes.
- The simulator ([sim](10-sim.md)) drives this crate under faults.
- `cargo test -p colophon-domain` compiles no HTTP, TLS or database stack.

## Build order

1. **P0:** IDs, scopes, schema field types, the rich-text validator, error types.
2. **P1:** entries, revisions and articles (step 1.2, done); the save rule in the simulator (1.4, done); content
   types (1.5, done); the role policy (1.7, done); slugs and locales.
3. **P2:** changesets and reviews (2.1, done); releases and publication history (2.2, done); job states (2.3, done).
4. **P3:** delivery states, evidence, unpublish.
5. **P4:** rollback, release-history queries, scheduling.

## Open decisions

- **O-005:** synchronous vs async ports, and generic vs `dyn`. The rust-mastery default is a generic core with `dyn` at
  the composition root; simulation favours synchronous logic.
- **O-006:** the content hash algorithm.
- The exact JSON Schema subset, and the v0.1 rich-text node allowlist.
