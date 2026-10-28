# 04 · api (`crates/colophon-api`)

> The management API, `/api/v1`. One surface for the Studio, the CLI, the SDK and agents, with no bypass.

| | |
|---|---|
| Milestones | P0 (skeleton, error envelope, health), P1 (projects, schemas, entries, revisions), P2 (changesets, reviews, releases, jobs), P3 (destinations, deliveries), P4 (assets, history), P5 (agents, credentials) |
| Proposed owner | Claude |
| Depends on | domain, auth, db (through ports), `axum` 0.8, `tower`, `tower-http`, `utoipa` for the OpenAPI document (D-027, from step 1.2) |
| Used by | studio, cli, sdk, automation; wired in by colophon-server |
| Spec | §09, §10, §26 |

## Why it exists

If the Studio, the CLI and agents each had their own path to the data, the rules would drift and one path would end
up privileged. A single API that authenticates, validates and calls the same domain services makes "the CLI and the
Studio produce identical records" something that can be tested, not hoped for (spec §10 parity gate).

## End goal

A stable `/api/v1` described by OpenAPI, with consistent errors, preconditions, idempotency and limits. The generated
client is tested against the server in CI, and a parity test proves the Studio, CLI and SDK running the same
create → conflict → review → publish scenario produce equivalent durable records.

## Responsibilities

- Routing and request validation.
- The middleware stack and the error envelope.
- Preconditions and idempotency handling.
- Pagination.
- The OpenAPI export.
- Rate limiting and admission.
- Health endpoints.

## Not its job

- Business rules, which live in [domain](01-domain.md).
- SQL, which lives in [db](02-db.md).
- Public content reads, which [delivery](06-delivery.md) serves at `/content/v1`.
- Rendering.

## Breakdown

### Server shape (rust-mastery chapter 12)
- Axum 0.8 routes with `/{param}` syntax.
- Handlers are thin: extract → authorize → call a domain service → map the result.
- One `ServiceBuilder`, outermost first (built in step 0.3, `crates/colophon-api/src/middleware.rs`):
  0. drop unsafe client request IDs (over 64 characters, or outside `[A-Za-z0-9._-]`);
  1. set request ID (UUIDv7);
  2. propagate request ID;
  3. trace (one span per request; per-request lines at DEBUG, so the default `info` level logs no line per request);
  4. error envelope: every 4xx/5xx becomes `{"error":{"code","message","request_id"}}`, including axum's 405 and
     tower-http's 413 and 504. Every 403 and 409 the handlers send also has `details.reason`, a stable code
     clients act on instead of the wording: `missing_permission` (with `details.permission`, such as
     `content.publish`), `cannot_grant`, `administrators_only`, `not_set_up`, `browser_session_required`,
     `origin_not_allowed`, `csrf_token_invalid`; and `already_published`, `not_approved`, `not_submitted`,
     `entries_moved_on`, `approval_expired`, `already_scheduled`, `schedule_published`, `schedule_blocked`,
     `schedule_cancelled`, `last_owner`, `already_member`, `owner_role_fixed`, `role_exists`, `project_exists`,
     `content_type_exists`, beside the older `stale_revision`, `stale_changeset`, `generation_moved` and the rest.
     A unit test refuses a refusal without a snake-case reason;
  5. body hygiene: never close a connection silently over an unread request body. Declared oversized bodies are
     refused after being drained, bodies a handler leaves unread are drained afterwards (up to 32 MiB, within 2 s),
     and otherwise the response says `Connection: close` (RFC 9112 §9.6; found by the testbed);
  6. `CatchPanicLayer::custom`;
  7. timeout (504 `deadline_exceeded`);
  8. body limit (1 MiB; uploads stream separately).
- `AppState` is cheap to clone (`Arc`s); there is no global mutable state.
- A fallback route returns the error envelope.

### Route families (spec §10, minus parked areas)

`{P}` means `/api/v1/projects/{project}/environments/{environment}`. Administrative access is denied by default.

| Route family | Operations and guard |
|---|---|
| `/api/v1/rich-text/validate` | The validator's verdict on a document: canonical form, or a 422 whose `details` name the rejection code and JSON Pointer. Stores nothing. Built in step 0.5; requires authentication from P1 |
| `/auth/*`, `/me` | OIDC login, callback and logout; session inspection and revocation; exact callback origins, CSRF. Built in step 1.1: `GET /auth/login`, `GET /auth/callback`, `POST /auth/logout`, and `GET /me` (the identity and its projects; since step 1.8 also its `subject` and whether it is an `administrator`, D-043). Since step 3.7 (D-061): `GET /auth/login?start=create` opens the provider's sign-up form when its discovery document lists `prompt=create`; `/me` adds `installation.has_administrator`; and `POST /api/v1/setup` (`{"code"}`, signed in, CSRF-checked) makes the caller the first administrator with the setup code the server printed, answering 422 `setup_code_invalid` (wrong, used or expired, alike), 409 `already_set_up`, or 429 `rate_limited` with `Retry-After` |
| `/api/v1/projects`, `/members`, `/invitations` | Project lifecycle and membership; owner-scoped. Built in step 1.1: list, create (with a `production` environment and the creator as owner; a taken slug is 409; administrators only since step 1.8, D-043) and get, where a project you aren't a member of is 404. Built in step 1.7 (D-042): `/members` (list; owners change roles and remove; anyone may leave) and `/invitations` (owners create, list and revoke), plus `POST /api/v1/invitations/lookup` and `/accept` with the token in the body; the last owner is 409 |
| `/api/v1/projects/{project}/tokens`, `/audit` | Built in step R.4 (D-063). `tokens`: list, create (`{"name", "description"?, "kind": "read-only"|"full-access"|"custom", "permissions"? (custom only), "expires_in_days": 1–366}`; 201 with `secret`, shown only there) and `DELETE …/tokens/{token}` (revokes; answers the token); `tokens.manage`, checked fresh; `cannot_grant`, `not_for_tokens`, `invalid_expiry`, `permissions_required`, `permissions_not_applicable`, `too_many_tokens`. `audit`: `?limit=&cursor=&action=&actor=`, newest first, `audit.read`. `Authorization: Bearer colophon_api_…` works only under its project's `/environments/` routes and `/api/v1/rich-text/validate`; another project is 404, other routes and `…/schedule` are 403 `api_token_not_allowed`, a revoked or expired token 401 |
| `{P}/schemas`, `/migrations` | Discover, validate, plan, apply, inspect; apply needs an approved current plan and migration permission. Built in step 1.5 as `{P}/content-types` (D-040): list, create, get with `ETag: "v<n>"`, and update with `If-Match`, accepted only for changes that keep stored entries valid (`needs_migration` otherwise); owners and developers only. Since step 3.6 (D-055) a narrowing change is checked against every draft and published revision: refused with 422 `needs_migration`, `details.entries` (the first 20 in the way, each with its title, `draft` or `published`, and the refusal its content would get) and `details.total`; more than 10,000 to check is `too_many_to_check`. A `slug` field (D-054): a save or create whose slug another entry has, as a draft or published, is 409 `slug_taken` naming it in `details.entries`; a release that would publish a slug another entry still publishes is 409 `slug_taken`. Published entries carry `slug` in `/content/v1` when they have one. Migrations later |
| `{P}/collections/{collection}/entries` | List and create; get, update and archive by ID; updates need the expected revision. Built in step 1.2 for the `articles` collection (D-034): list (cursor), create, get with `ETag`, save with `If-Match`, and `…/{entry}/revisions`; archive later. A read says whether the entry is live: `published` is `{revision_id, generation}`, or `null` when it isn't published (create and save answers leave it out, since they don't look it up). List summaries carry `created_by`, as reads do |
| `{P}/entries/{id}/revisions`, `/comments` | History, compare, restore as a new draft, revision-bound comments |
| `{P}/changesets`, `/reviews`, `/releases` | Prepare, validate, request review, approve, publish, schedule, cancel, inspect, roll back as a new generation. Built in step 2.1 (D-044): `{P}/changesets` list and create (idempotent; 1 to 100 entries, each pinned at its current revision), `{P}/changesets/{id}` with `ETag: "sha256:…"`, and `…/submit`, `…/refresh` and `…/reviews` (`{"decision", "comment"}`, owners and publishers), each needing `If-Match` with the hash (409 `stale_changeset` otherwise). Built in step 2.2 (D-045): `…/publish` (`{"base_generation"}`, `If-Match` and `Idempotency-Key`; 409 `generation_moved` if another release committed first) and `{P}/releases`, newest first, with `Colophon-Generation`. Built for D-060: `…/schedule` (`{"local_time", "time_zone", "choice"?}`, `If-Match`; 422 `ambiguous_local_time` or `skipped_local_time` with `details.candidates`, `in_the_past`, `too_far_ahead`, `after_approval_expires`, `unknown_time_zone`, `invalid_local_time`; 409 if one is pending), `{P}/schedules` (`?changeset=`; pending ones, then the 20 latest finished, each with `delayed`) and `DELETE {P}/schedules/{id}` (its author, or anyone who may publish it; 409 once it ran). Publish request summaries carry `created_by` and `submitted_by`, so a list can show someone their own requests and those waiting on them |
| `{P}/assets`, `/upload-intents` | Create an intent, complete verification, inspect, list references, request deletion |
| `{P}/agents`, `/jobs` | Scoped credentials, proposals, bounded bulk work, status, authorized cancellation |
| `{P}/destinations`, `/deliveries` | Configure and test integrations; inspect failures; retry or reconcile safely. Built in step 2.3 (D-046): `{P}/destinations` (list and create, owners and developers; https and public addresses), `PUT {P}/destinations/{id}` (`{"enabled"}`), and `{P}/jobs?generation=` for any member |

The spec's `/forms`, `/leads`, `/commerce/*` and `/store/v1` families are parked ([parked](parked.md)).

### Conventions (spec §09, §10)
- **Identifiers and time:** opaque IDs; request IDs on every response; inputs in UTC RFC 3339.
- **Cursor pagination:**
  - default page size 20, maximum 100;
  - a stable ID tie-breaker;
  - cursors are bound to the project and query shape, and rejected elsewhere.
- **Preconditions:** single-resource updates use `If-Match` with revision ETags. A missing precondition returns
  **428**; a stale one returns **409** with a safe description of the conflict. As built (D-035): the ETag is
  `"<revision id>"`; a malformed `If-Match`, or `*`, is 400; the 409 has `details.reason` `stale_revision` and the
  current `ETag` header.
- **Idempotency:** retriable creates need `Idempotency-Key`, scoped to actor + operation + environment. A matching
  replay returns the recorded result; a different payload conflicts. As built (D-035): saves need a key too; keys are
  16 to 128 of `A-Z a-z 0-9 - _`; a replay carries `Idempotent-Replayed: true`; a reused key is 409 with
  `details.reason` `idempotency_key_reused`; only successes are recorded.
- **Response shapes** (as built, D-035: `{ data }` and `{ data, next_cursor }`, with the request id in the
  `x-request-id` header and the revision in `ETag`):
  - Success: `{ data, request_id, revision }`.
  - Asynchronous work: `202 { operation_id, status: "accepted", poll_url }`. It never says published.
  - Lists: `{ data, next_cursor, filters, limits }`.
  - Errors: `{ code, message, fields[], retryable, request_id }`, with no stack traces or secrets.
- **Allowlists:** only documented filters, sorts and projections are accepted. Relation expansion depth is at most 2,
  and ordinary queries have a 2 s deadline.

### Status codes

| Code | Meaning |
|---|---|
| 401 | Missing or invalid identity |
| 403 | Action denied for a visible resource |
| 404 | Missing, or hidden by the visibility policy |
| 409 | Conflict: stale revision or generation, or a different payload for an idempotency key |
| 413 | Body over the limit |
| 422 | Invalid content, with field paths |
| 428 | Required precondition missing (`If-Match`, `Idempotency-Key`) |
| 429 | Throttled (per-principal rate limit) |
| 503 | Dependency or capacity unavailable, with `Retry-After` |

### Contracts and discovery
- An OpenAPI document is exported from the code (D-027, `utoipa`). The TypeScript client ([sdk](13-sdk.md)) is generated from it
  and tested against the server in CI.
- Schema discovery returns only the collections and fields the caller may access.
- Client-side validation helps feedback, but Rust revalidates every mutation.
- Contract changes need compatibility tests and a documented deprecation window.

### Admission and health
- **Admission:**
  - per-principal rate limits (`governor`);
  - pool and queue admission returns 503 before an unbounded queue forms;
  - probes are never throttled or authenticated.
- **Health:**
  - `/livez` means the process responds;
  - `/readyz` reports whether this role can safely serve. A cache outage can be "degraded" rather than unready, while
    a database outage makes writes unavailable.

## Invariants

| ID | Invariant |
|---|---|
| API1 | Every mutation carries actor, request ID and authorization context, and goes through a domain service. |
| API2 | `202` means accepted, never published or completed. |
| API3 | Updates without a precondition are refused (428); stale preconditions never overwrite (409). |
| API4 | The same idempotency key and payload return the same result; a different payload conflicts. |
| API5 | Every error response is the JSON envelope with a `request_id`, including middleware-generated ones. |
| API6 | No response contains a stack trace, secret, internal address or another project's data. |

## Interfaces

- HTTP: `/api/v1/*`, `/auth/*`, `/me`, `/livez`, `/readyz`.
- The OpenAPI document (a build artifact).
- `router(state) -> Router`, used by colophon-server.

## Failure modes

| Situation | Behaviour |
|---|---|
| Database down | 503 with `Retry-After` once the pool's acquire timeout passes (D-033); nothing is acknowledged |
| Request exceeds a limit | 413 or 422 with field paths; nothing committed |
| Handler panic | `CatchPanicLayer::custom` returns the JSON envelope (500); logged once |
| Slow dependency | Timeout layer returns 504 within the request budget |
| Overload | 429 per principal, or 503 with `Retry-After` |

## Tests and evidence

- In-process `oneshot` tests for every route and every error class.
- Contract tests of the OpenAPI document against the server.
- The parity gate: Studio-API, CLI and SDK run the same scenario and produce equivalent records.
- Cursor fuzzing: tampered cursors, cursors from another project or query shape.
- A middleware-order test: request ID present on panic, timeout and body-limit responses.
- A pool that can't hand out a connection gives 503 with `Retry-After` and the envelope (`tests/busy.rs`).
- Entries end to end against the real binary (`crates/colophon-server/tests/entries.rs`), and the crash tests F01 and
  F02 (`faults.rs`, D-036).
- Members and invitations end to end with two people (`crates/colophon-server/tests/members.rs`, D-042); every
  route asks the one policy table (`colophon_domain::policy`).
- Changesets end to end (`crates/colophon-server/tests/changesets.rs`, D-044): an approval bound to the exact hash,
  an edit making the changeset stale, a refresh re-pinning it, decisions in the same second ordered correctly.

## Build order

1. **P0:** skeleton, error envelope, request IDs, health.
2. **P1:** projects (step 1.1, done); entries, revisions, preconditions, idempotency and pagination (step 1.2,
   done); content types (step 1.5, done); members and invitations (step 1.7, done); environments, migrations, and the generated OpenAPI document.
3. **P2:** changesets and reviews (step 2.1, done); releases (step 2.2, done); destinations and jobs (step 2.3, done).
4. **P3:** destinations and deliveries.
5. **P4:** assets, history and rollback.
6. **P5:** agents and machine credentials.

## Open decisions

- Whether the Studio (session cookie) and the CLI (bearer credential) share every route, or whether some routes are
  cookie-only.
- Rate-limit defaults per role.
