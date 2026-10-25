# 13 · sdk (`packages/sdk`)

> A generated TypeScript client with generation-pinned reads, so websites can read content consistently across one
> release.

| | |
|---|---|
| Milestones | P2 (generated client, pinned-read helper), P3 (used by the blog and Studio) |
| Proposed owner | Claude |
| Depends on | the OpenAPI document exported by [api](04-api.md) |
| Used by | studio, web/blog template, external sites |
| Spec | §09, §10, §14 |

## Why it exists

A website often makes several related reads to build one page: a post, its author, its category. If a release lands
between those reads, the page mixes two releases. The SDK pins one `(epoch, generation)` across related reads, and it
removes hand-written API code that would drift from the server.

## End goal

A generated, typed client for `/api/v1` and `/content/v1`, tested against the real server in CI, with a helper that
pins a generation across related reads and safe retry behaviour for mutations.

## Responsibilities

- Generated types and calls.
- The pinned-read helper.
- Idempotent retries.
- Typed errors.
- Versioning in step with the API.

## Not its job

- Business rules or validation authority.
- Caching policy, which belongs to [delivery](06-delivery.md).

## Breakdown

- **Generated client:** produced from the OpenAPI document (generator chosen with O-004).
- **Pinned reads:** `withRelease(client)` resolves the active `(epoch, generation)` once, then reads everything at
  that release.
- **Mutations:**
  - every retriable mutation carries one idempotency key, reused across retries;
  - updates send `If-Match`;
  - errors are typed from the error envelope (`code`, `fields`, `retryable`, `request_id`).
- **Versioning:** the SDK is versioned with the API, with a documented deprecation window for contract changes
  (spec §10).

## Invariants

| ID | Invariant |
|---|---|
| K1 | Reads made through one pinned scope always use one `(epoch, generation)`. |
| K2 | A retried mutation reuses its idempotency key. |

## Interfaces

- `createClient(options)`, `withRelease(client)`, plus generated resource methods.

## Failure modes

| Situation | Behaviour |
|---|---|
| Pinned generation becomes unavailable (revoked content) | Typed error; the page decides what to show; no silent fallback to another release |
| Transient failure on a mutation | Retry with the same key, within the caller's deadline |

## Tests and evidence

- Generated-client tests against the server in CI.
- A pinned-read test with a publish landing mid-sequence.
- Doc examples compiled in CI.

## Build order

1. **P2:** generation pipeline and pinned reads.
2. **P3:** adoption by the blog and the Studio.

## Open decisions

- **O-004:** the generator, which follows the OpenAPI crate choice.
