# 06 · delivery (`crates/colophon-delivery`)

> Public reads, time travel, caching, revocation and proof. Readers get only published content, quickly, and every
> release can show where it was actually observed.

| | |
|---|---|
| Milestones | P2 (public API from Postgres, pinned reads), P3 (cache, CDN, tags, probes, unpublish), P4 (time-travel support, preview, sitemaps, feeds) |
| Proposed owner | Claude builds; publishing and revocation transitions get a manual review |
| Depends on | domain (published view, publication history, delivery states), db (the delivery role and published projections), a Redis-compatible client (O-002), the shared HTTP client |
| Used by | readers via the CDN, sdk, web/blog template, worker (purge and probe jobs), studio (receipts) |
| Spec | §06, §14, §15, §25 |

## Why it exists

The public read path is where private data most easily leaks (a draft in a shared cache) and where "published" most
easily lies (a CMS row changed, but the website didn't). Delivery owns both problems:
- it serves only published projections;
- it keys every cache entry by release generation;
- it treats every success claim as an observation with evidence.

## End goal

Public reads come from the right generation, are cached safely, invalidated correctly and revocable. Pinned reads
never mix releases, and every release has scoped evidence of where its marker was observed. Proven by F06–F08, L01–L04,
L10, L11, L13, the spec §27 cache-isolation tests and the mixed-release checker.

## Responsibilities

- The public content API, `/content/v1`.
- Pinned (time-travel) reads.
- Cache keys and the cache layer.
- CDN headers and tag purge.
- Unpublish and revocation.
- Evidence probes.
- The preview origin.
- Sitemaps, feeds and redirects.
- Locale fallback.

## Not its job

- Editorial or management reads. Those go through [api](04-api.md).
- Deciding what is published. The release commit in [domain](01-domain.md) and [db](02-db.md) does that.
- Running external calls inline. Purges and probes are worker jobs.

## Breakdown

### Public content API (spec §10, §15)
- `/content/v1/{project}/{env}/…` lists and gets collections and entries from **published projections only**.
- It uses the `colophon_delivery` database role, which cannot read draft tables.
- It reads the active generation by default, or a pinned one with `?generation=` (time travel).
- Responses include the generation, an ETag and the resolved locale.
- A pinned request for revoked or inaccessible content is still denied: history never bypasses current revocation
  rules (spec §10).

### Cache keys (spec §15)
- A key includes:
  - project, environment, recovery epoch, release generation and locale;
  - schema/render version;
  - the normalized query and the response representation.
- Only allowlisted filters and sorts are cacheable, so unbounded query variants are never cached.
- On an origin miss, the active generation is always resolved from Postgres, so a stale cache pointer can never
  reintroduce an older publication.
- Generations from different epochs are never interchangeable.

### Cache layer (Redis-compatible)
- Disposable, generation-keyed public results: 300 seconds with jitter, a 1 GiB reference budget, explicit eviction.
- Request coalescing (single-flight) on misses, so 1,000 callers at expiry trigger one regeneration (L04).
- When the cache is down, reads fall back to Postgres with coalescing and admission limits, never unlimited database
  load (L11).

### CDN headers and purge (spec §15)

| Response | Policy |
|---|---|
| Active-release metadata and live public queries | Shared TTL 30 seconds |
| Generation-pinned public content | Shared TTL 300 seconds |
| Browser freshness for content | 0, with revalidation |
| Preview, drafts, sessions, personalized responses | `private` / `no-store` |
| Content-removal paths | Stale-on-error disabled |

These are policies to test, not a global 30-second consistency guarantee.

- Tags cover entry, collection, locale and known references, and include collection listings even when a newly
  matching entry was absent from the old result (F06). Invalidation starts at collection level and is optimized only
  after correctness tests pass.
- The Cloudflare adapter purges by tag within documented plan limits. A successful purge response and sampled reads are
  different evidence (spec §15).

### Unpublish and revocation (spec §14, §15)
- Unpublish writes a tombstone and marks the entry revoked for origin access, including historical generation URLs.
- It then purges all known paths and derivatives.
- The UI shows *removal pending* until the configured checks pass.
- Browser caches and copies already downloaded cannot be recalled, and the docs say so.

### Evidence probes (spec §14)
- Probes fetch configured URLs from configured locations and look for the release's generation marker or response hash.
- Evidence records the generation, marker or hash, observation location, timestamp and errors.
- Observations are scoped: with one probe location the UI says *verified at 1 location*, never *verified* (O-012).
- A fresh API response does not prove that a statically generated page rebuilt. The site adapter checks the deployed
  page's own marker ([web](15-web-blog.md)).

### Preview origin (spec §06)
- A separate, sandboxed origin validates the 15-minute capability ([auth](03-auth.md)) on every protected fetch.
- Responses are `no-store` and `noindex`, with a restrictive referrer policy.
- Preview artifacts never enter public CDN routes, sitemaps or search indexes.

### Site metadata (spec §06)
- Sitemaps, feeds and language alternates are generated from a single published generation.
- Canonical URLs are normalized against the verified site configuration.
- A slug change proposes a reviewed redirect, with cycles and collisions rejected.
- Locale fallback follows an explicit project policy: none by default; it resolves only to published revisions and
  labels the resolved locale.

## Invariants

| ID | Invariant |
|---|---|
| DL1 | No draft, preview or private field ever appears in a public response or shared cache. |
| DL2 | A pinned read set never mixes generations. |
| DL3 | The active generation is resolved from Postgres on an origin miss; the cache can never reintroduce an older release. |
| DL4 | Cache entries from different epochs never collide. |
| DL5 | Revoked content is denied at the origin, including through historical generation URLs. |
| DL6 | Evidence is always scoped to the locations actually probed; no claim of global verification. |
| DL7 | Cache loss never causes data loss or a permission bypass. |

## Interfaces

- HTTP: `/content/v1/{project}/{env}/…` and the preview origin.
- Purge and probe adapters used by [worker](05-worker.md) jobs.
- Cache-key and tag builders shared with the site build adapter.

## Failure modes

| Situation | Behaviour |
|---|---|
| Cache unavailable for 5 minutes (L11, F08) | Bounded Postgres fallback with coalescing; nothing lost; gradual recovery |
| Hot key expires under load (L04) | Single regeneration; no permission-crossing reuse |
| CDN purge 429 / 5xx (L13) | Backoff; delivery stays pending or uncertain; no claim of success |
| In-flight stale fill after a publish | Generation keys prevent poisoning the new generation's entries |
| Postgres slow | Bounded pools, 503 with `Retry-After`, eligible public cache served within TTL |

## Tests and evidence

- **Fault cases:**
  - F06: a new matching article invalidates listings.
  - F07: unpublish while old caches exist.
  - F08: cache eviction, failover or restart.
- **Load cases:** L01–L04, L10 (warm CDN plus repeated publishes, mixed-release detection), L11, L13.
- **Cache isolation (spec §27):**
  - the same slug in two projects;
  - a private response is never reused publicly;
  - locale and query-key collisions;
  - a stale fill after publish;
  - removal of prior public versions.
- The mixed-release consistency checker ([sim](10-sim.md)).

## Build order

1. **P2:** the public API from published projections without a cache; pinned reads.
2. **P3:** cache layer, CDN headers, tags, Cloudflare purge, probes, unpublish and revocation.
3. **P4:** time-travel queries for the Studio slider, the preview origin, sitemaps, feeds and redirects.

## Open decisions

- **O-002:** Redis or Valkey.
- **O-012:** probe locations for v0.1.
- How to implement single-flight coalescing across multiple API processes, if they are ever split.
