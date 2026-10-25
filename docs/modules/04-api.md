# 04 · api (`crates/colophon-api`)

> The management API, `/api/v1`. One surface for the Studio, the CLI, the SDK and agents, with no bypass.

| | |
|---|---|
| Milestones | P0 (skeleton, error envelope, health), P1 (projects, schemas, entries, revisions), P2 (changesets, reviews, releases, jobs), P3 (destinations, deliveries), P4 (assets, history), P5 (agents, credentials) |
| Proposed owner | Claude |
| Depends on | domain, auth, db (through ports), `axum` 0.8, `tower`, `tower-http`, an OpenAPI crate (O-004) |
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
     tower-http's 413 and 504;
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
| `/auth/*`, `/me` | OIDC login, callback and logout; session inspection and revocation; exact callback origins, CSRF |
| `/api/v1/projects`, `/members`, `/invitations` | Project lifecycle and membership; owner-scoped |
| `{P}/schemas`, `/migrations` | Discover, validate, plan, apply, inspect; apply needs an approved current plan and migration permission |
| `{P}/collections/{collection}/entries` | List and create; get, update and archive by ID; updates need the expected revision |
| `{P}/entries/{id}/revisions`, `/comments` | History, compare, restore as a new draft, revision-bound comments |
| `{P}/changesets`, `/reviews`, `/releases` | Prepare, validate, request review, approve, publish, schedule, cancel, inspect, roll back as a new generation |
| `{P}/assets`, `/upload-intents` | Create an intent, complete verification, inspect, list references, request deletion |
| `{P}/agents`, `/jobs` | Scoped credentials, proposals, bounded bulk work, status, authorized cancellation |
| `{P}/destinations`, `/deliveries` | Configure and test integrations; inspect failures; retry or reconcile safely |

The spec's `/forms`, `/leads`, `/commerce/*` and `/store/v1` families are parked ([parked](parked.md)).

### Conventions (spec §09, §10)
- **Identifiers and time:** opaque IDs; request IDs on every response; inputs in UTC RFC 3339.
- **Cursor pagination:**
  - default page size 20, maximum 100;
  - a stable ID tie-breaker;
  - cursors are bound to the project and query shape, and rejected elsewhere.
- **Preconditions:** single-resource updates use `If-Match` with revision ETags. A missing precondition returns
  **428**; a stale one returns **409** with a safe description of the conflict.
- **Idempotency:** retriable creates need `Idempotency-Key`, scoped to actor + operation + environment. A matching
  replay returns the recorded result; a different payload conflicts.
- **Response shapes:**
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
| 428 | Required precondition missing |
| 429 | Throttled (per-principal rate limit) |
| 503 | Dependency or capacity unavailable, with `Retry-After` |

### Contracts and discovery
- An OpenAPI document is exported from the code (O-004). The TypeScript client ([sdk](13-sdk.md)) is generated from it
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
| Database down | Writes return 503 honestly; nothing is acknowledged |
| Request exceeds a limit | 413 or 422 with field paths; nothing committed |
| Handler panic | `CatchPanicLayer::custom` returns the JSON envelope (500); logged once |
| Slow dependency | Timeout layer returns 503 within the request budget |
| Overload | 429 per principal, or 503 with `Retry-After` |

## Tests and evidence

- In-process `oneshot` tests for every route and every error class.
- Contract tests of the OpenAPI document against the server.
- The parity gate: Studio-API, CLI and SDK run the same scenario and produce equivalent records.
- Cursor fuzzing: tampered cursors, cursors from another project or query shape.
- A middleware-order test: request ID present on panic, timeout and body-limit responses.

## Build order

1. **P0:** skeleton, error envelope, request IDs, health.
2. **P1:** projects, environments, schemas, entries, revisions, preconditions, idempotency, pagination.
3. **P2:** changesets, reviews, releases, jobs.
4. **P3:** destinations and deliveries.
5. **P4:** assets, history and rollback.
6. **P5:** agents and machine credentials.

## Open decisions

- **O-004:** the OpenAPI generation crate.
- Whether the Studio (session cookie) and the CLI (bearer credential) share every route, or whether some routes are
  cookie-only.
- Rate-limit defaults per role.
