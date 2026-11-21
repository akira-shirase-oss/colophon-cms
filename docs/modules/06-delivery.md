# 06 · delivery (`crates/colophon-delivery`)

> Public reads, time travel, caching, revocation and proof. Readers get only published content, quickly, and every
> release can show where it was actually observed.

| | |
|---|---|
| Milestones | P2 (public API from Postgres, pinned reads), P3 (cache, CDN, tags, probes, unpublish), P4 (time-travel support, preview, sitemaps, feeds) |
| Proposed owner | Claude builds; publishing and revocation transitions get a manual review |
| Depends on | domain (published view, publication history, delivery states), db (the delivery role and published projections), a Redis-protocol client (Valkey, D-015), the shared HTTP client |
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
- Preview reads, `/preview/v1` (D-172).
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
- Built in step 2.2 (D-045): `GET /content/v1/{project}/{environment}/{collection}?generation=&limit=&cursor=` and
  `…/{collection}/{entry}`, with no sign-in, from `publications` as `colophon_delivery` (`colophon_db::delivery`,
  which switches role for each transaction). Answers carry `Colophon-Generation`; a list's cursor keeps the
  generation of its first page; a generation that hasn't committed is 422 `invalid_generation`. Projects are
  addressed by id; locales come later.
- Step 3.1 (D-048) adds a strong `ETag` (304 for a matching `If-None-Match`), the cache policies below,
  `Cache-Tag`, and `Colophon-Cache: hit | miss | coalesced | bypass`.
- Step 3.2 (D-049): revoked publications are never delivered; an unpublished entry is 410 `gone`.
- P7 (D-139) adds the public live-hint stream:
  `GET /content/v1/{project}/{environment}/events` is an anonymous server-sent-events stream that says
  only *that* published content changed — no ids, kinds or collections, so a gated entry's existence
  cannot leak through it. Only `releases` hints are forwarded (a publish inserts a release row; draft
  saves and delivery-job churn stay off the stream), a burst coalesces into one `change` frame, and the
  stream is bounded by a budget of its own (D-159), apart from the staff bus's (D-108): 256 public streams a
  project and 2048 in all on one server (`503` past either), 8 per client address (`429` with `Retry-After`;
  `COLOPHON_EVENTS_PUBLIC_PER_PROJECT`, `_PUBLIC_MAX_STREAMS`, `_PUBLIC_PER_ADDRESS`) — so anonymous
  listeners can never lock a project's editors out of their streams — ending at its lifetime. A site reacts by re-reading through the delivery API at the new generation: The Draft renders
  its public pages per request and fans one upstream connection out to every open tab, so a publish
  reaches a browser with no rebuild.
- DX.4 (D-103, D-105) adds `POST /content/v1/{project}/{environment}/{collection}/query?generation=&cursor=`:
  the bounded query language of [domain](01-domain.md) over the published projection, still anonymous. The
  body is the query document the draft route takes; `?generation=` pins the release, `?cursor=` a page
  boundary sealed `q1:<generation>:<v1 token>` — the generation of its first page, then the query-bound
  `v1:` token, so paging never mixes two releases (DL2). Rows are the `publications` projection pinned by
  delivery's own predicate — `valid_from` open at the generation, never revoked — so unpublished and
  tombstoned entries never answer at any generation (DL5). `fields` narrows the returned content; `depth`
  populates (DX.5, D-107): each level's targets resolve from `publications` under the page's own pin,
  inside the fill's transaction — so a populated reference reads exactly the release its row does, and a
  publication revoked or taken down since resolves to nothing, staying the stored id at any generation.
  `applied` echoes the canonical
  document and its `query_hash`, and publications carry no locale, so every answer is in the stored one until
  locale lands on the projection.

- Gap 3 (D-178) adds `media` to every row (list, get and query), left out when the published revision names no
  uploads: by asset id, the version of each upload — asset fields and rich-text images alike — that the row's
  release recorded in `publications.media`. Each carries `url` (the versioned address
  `{COLOPHON_MEDIA_URL}/media/v1/{project}/{env}/{asset}/{sha}`, immutable and cached a year, D-157),
  `renditions` (`w320`, `w1280` under that version, only those the worker wrote), `width`, `height`, `type`, and
  `current_url` (the bare address, which shows the public the newest published version, D-229; in a signed-in site
  member's answer it is `url` itself, because a member's signed address opens only a version a release recorded).
  A site that uses `url` shows exactly the bytes
  that were released, so a pinned read at generation N shows N's images; a replaced image reaches it when the
  entries that use it are published again (publishing an unchanged entry whose upload has a newer version reopens
  its publication). A site that wants the newest published bytes without republishing every entry uses
  `current_url`; a replacement no release has named yet reaches nobody but the project's members and previews.
  `COLOPHON_MEDIA_URL` (an http(s) origin, default `COLOPHON_PUBLIC_URL`) puts a CDN or asset host in front.
  On a query, a row's `media` is what its answered `content` names: `fields` narrowing drops the stamps of the
  fields it left out, and `depth` adds the versions each populated reference's own release recorded (an avatar on
  a populated author), so a site renders a populated reference's images at their recorded versions too.

### Gated content (P7, D-137)
- Every draft, revision and publication carries a `visibility`: `public` (the default), `authenticated` (any
  signed-in site user) or `role:<name>` (a site user whose [audience](03-auth.md) role is `<name>`). Editorial
  writes take it as `?visibility=`; `role:` validates against the project's live audience roles, so a gate can
  never name a role that doesn't exist. The gate pins with the revision like the content itself — a release
  publishes whatever gate the pinned revision carries.
- Enforcement is row-level security, not a filter the code could forget: a delivery transaction `SET LOCAL ROLE
  colophon_delivery` and names the reader as `app.audience_role` — unset for anonymous, the live session's role
  for a member. The `publications` policy answers `visibility = 'public'` rows to anyone, and
  `visibility = 'authenticated'` or `visibility = 'role:' || current_setting('app.audience_role')` rows to a
  member; a gated row the caller's role doesn't match is the same 404 as an unpublished one, to them and to
  their `?generation=` pins.
- A request's audience resolves *before* the cache: a live site session (bearer or `colophon_audience` cookie)
  makes the read a member's — answered `Cache-Control: private, no-cache` with `Colophon-Cache: bypass`
  straight from Postgres (its own ETag still earns a 304), and **never** through the generation-keyed cache,
  whose keys name no role and whose audience is anonymous by construction. A dead or absent credential is
  simply anonymous — delivery never 401s a public collection, it answers the public rows, and the member
  answer's `Vary: Authorization, Cookie` keeps any shared cache between the two honest.

### Cache keys (spec §15)
- A key includes:
  - project, environment, recovery epoch, release generation and locale;
  - schema/render version;
  - the normalized query and the response representation.
- Only allowlisted filters and sorts are cacheable, so unbounded query variants are never cached.
- On an origin miss, the active generation is always resolved from Postgres, so a stale cache pointer can never
  reintroduce an older publication.
- Generations from different epochs are never interchangeable.
- Built in step 3.1 (D-048): `colophon_domain::cache::Scope` (render version, project, environment, epoch,
  generation; then limit, cursor position and the collection, last). Ids are fixed-width hex, so the collection name
  is the only free text in a key. Unknown query parameters don't change an answer and aren't in its key. Each request
  resolves its generation from Postgres in one statement, before and apart from the cache: `delivery_environment`
  (migration 0011) runs as `colophon_delivery` in the project's context and returns only the environment's id, epoch
  and generation. As a delivery transaction (five round trips) the lookup cost as much as the cache saved.
- Gap 3 (D-178): `RENDER_VERSION` is 2 (rows carry `media`), and the key's prefix names the media origin's
  digest (`m{16 hex}`), since answers carry absolute media URLs: an answer cached while media pointed at one
  origin is never served after `COLOPHON_MEDIA_URL` moves.
- DX.4 (D-105): a query page keys on `Scope::query_key` — the normalized query's own `query_hash` and the page
  boundary's SHA-256, both fixed-width hex, the collection last as always — so two different queries can never
  share an answer (§15) and a key stays short however long a cursor is. The boundary names the canonical
  position re-sealed, not the request's token: one page, one key. A query page takes the list's tags, so a
  purge finds it like any listing.

### Cache layer (Redis-compatible)
- Disposable, generation-keyed public results: 300 seconds with jitter, a 1 GiB reference budget, explicit eviction.
- Request coalescing (single-flight) on misses, so 1,000 callers at expiry trigger one regeneration (L04).
- When the cache is down, reads fall back to Postgres with coalescing and admission limits, never unlimited database
  load (L11).
- Built in step 3.1 (D-048): `colophon_api::DeliveryCache`. Valkey calls have a 100 ms deadline. A miss is filled
  once per key per process (the leader looks once more first), and waiters share the outcome, failures included.
  Empty pages, 404s and answers over 1 MiB are never stored, so made-up names in URLs can't fill the cache. After a
  failure the connection is dropped and left alone for a second; one request reconnects at a time, and the rest read
  Postgres, whose pool (and 503 with `Retry-After` when it's full) is the admission limit.

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
- Built in step 3.2, part 2 (D-050): CDN destinations and purge jobs. A purge names each changed entry's tag and its
  collection's (at most 30 tags, else the environment's), covers every release since that CDN's last successful purge,
  and the releases list reports each release's purge state. The Cloudflare adapter (credentials, plan limits) is
  step 3.4.

### Unpublish and revocation (spec §14, §15)
- Unpublish writes a tombstone and marks the entry revoked for origin access, including historical generation URLs.
- It then purges all known paths and derivatives.
- The UI shows *removal pending* until the configured checks pass.
- Browser caches and copies already downloaded cannot be recalled, and the docs say so.
- Built in step 3.2, part 1 (D-049): unpublishing is a changeset item of kind `unpublish` that pins the published
  revision and is reviewed like any change. Its release closes the publication, records a tombstone, and marks every
  earlier publication of the entry `revoked_in`; delivery never serves a revoked publication, and a read of the entry
  is 410 `gone` (`no-store`) at every generation it was published in and at every one from the takedown on, until it
  is published again. At a generation from before the entry was first published it is 404 `not_found`, as for any
  entry that didn't exist then: there was nothing to take down. Cache keys name the environment's
  `last_unpublish`, so answers cached before a takedown are never served again (F07 at the origin,
  `crates/colophon-server/tests/cache.rs`). Part 2 (D-050) purges it from CDNs: the release reports its purge as
  pending, accepted or failed, and the Studio shows *removal pending at the CDN* until it is accepted
  (`crates/colophon-server/tests/purges.rs`, against a fake CDN that caches by `Cache-Tag`).

### Evidence probes (spec §14)
- Probes fetch configured URLs from configured locations and look for the release's generation marker or response hash.
- Evidence records the generation, marker or hash, observation location, timestamp and errors.
- Observations are scoped: with one probe location the UI says *verified at 1 location*, never *verified* (O-012).
- A fresh API response does not prove that a statically generated page rebuilt. The site adapter checks the deployed
  page's own marker ([web](15-web-blog.md)).
- Built in step 3.3 (D-051): probe destinations. A probe reads the `Colophon-Generation` header or a
  `<meta name="colophon-generation" content="N">` marker, passes on that release or a later one, and leaves an
  observation for every look (location, generation seen, status, body hash, error). Workers have a location
  (`COLOPHON_WORKER_LOCATION`), and a probe may be pinned to one. Each release has one label from
  `colophon_domain::receipt::label`, and `GET …/releases/{generation}` is its receipt
  (`crates/colophon-server/tests/receipts.rs`, against a fake site; verified at 2 locations with a second worker),
  read in one snapshot (`scope::Isolation::Snapshot`, REPEATABLE READ) so the deliveries and the locations that saw
  them always describe the same moment.

### Preview reads (D-172; built 2026-11-17, gap 1)
`/preview/v1` answers exactly what `/content/v1` answers, with a grant's scope ([auth](03-auth.md), D-171) laid over
the latest release. It deviates from spec §06's separate preview origin: Colophon serves JSON, not pages — the page
sandbox is the customer's own site — so preview is a path family on the content origin, which an operator may still
route to its own host.
Preview rows carry `media` like delivery's (D-178): a published row the versions its release recorded, a draft row
the current versions of the uploads it names (as drafts show them), kept to what the issuer's role may see.

- `POST /preview/v1/{project}/{environment}/exchange {code}` → the bearer token;
  `GET …/session` → scope and end, for the site's banner;
  `GET …/{collection}?limit=&cursor=`, `GET …/{collection}/{entry}`, `POST …/{collection}/query?cursor=`.
- **The overlay.** For the latest generation G: every publication open at G, not revoked, whose entry the scope
  doesn't name — plus each scope row that shows a revision of a live entry of the collection (the entry's current
  draft, or the request's pinned revision); a scope row without one (an unpublish item) takes the entry out, and a
  deleted entry is invisible to the API role so it can't be in scope. One statement head,
  `colophon_db::query::OVERLAY`, serves list, get and the query language (its third source, `overlay_page`), so the
  three can't disagree; it runs as `colophon_api` in the issuer's transaction, never as `colophon_delivery`.
  `colophon_domain::preview::overlay` is the same rule as a pure function, the model the scope property test checks
  the SQL against.
- **The shape.** Each row is delivery's own `PublishedView` (one serializer for both) plus
  `preview: {source: "published" | "draft" | "changeset", hidden_fields?}`; for a scope row `published_in` is G, the
  release the preview is laid over. A published row reads byte for byte as `/content/v1` reads it (`tests/preview.rs`).
  When the locale (gap 2) and media (gap 3) shapes land in `PublishedView`, preview carries them unchanged.
- **Never cached.** Every answer — refusals included — is `Cache-Control: private, no-store`,
  `X-Robots-Tag: noindex, nofollow, noarchive`, `Referrer-Policy: no-referrer`, `Vary: Authorization`, with
  `Colophon-Preview: entry | changeset` and `Colophon-Generation: G`; never `ETag`, never `Cache-Tag`, never the
  delivery cache. Operators bypass `/preview/*` at the CDN.
- **Refusals.** 401 `preview_invalid | preview_expired | preview_revoked` with `WWW-Authenticate: Bearer
  error="invalid_token"`; 404 for another environment or an unknown entry; 409 `preview_stale`; 410
  `preview_published`; 422 `preview_reads_latest` (`?generation=`) and `preview_depth` (`depth` population isn't
  served in preview yet); 503 with `Retry-After` past the per-server bound of 32 concurrent preview reads.
- **Bounds.** `/preview/` counts in admission per client address and per bearer (D-076's management rates); the
  semaphore keeps a burst of previews from taking the connections delivery needs; the exchange reads at most 1 KiB.
- **Browsers.** A static or client-rendered site may exchange and read from the browser: `/preview/v1` answers
  CORS for exactly the environment's preview-address origin (`Access-Control-Allow-Origin` echoed, `Authorization`
  and `Content-Type` allowed, `GET` and `POST`, never `Allow-Credentials` — preview reads are bearer, never cookie;
  `Vary: Origin`). Any other origin gets no CORS headers, so the browser keeps the answer from its page. Preflights
  are answered before the router; a request without `Origin` (a site's server) costs nothing extra.
- **Load.** `perf/k6/preview.js` (entry and publish-request scopes, a query, invalid and revoked tokens, an
  exchange arm), smoke-run in CI at 200/s for 20 s: on a GitHub runner (2026-11-17, branch CI) 225/s, p99 62 ms,
  no failures — inside the spec's 150 ms budget, but about ten times delivery's cached p99 on the same runner, since
  every preview read checks its grant and opens as the issuer in two transactions. A runner observation, not a
  capacity claim. Owed: the full 1M run, the stress run (knee; the failure mode must be the bound's 503) and the
  mixed run beside `delivery.js`, on a quiet machine.
- **Not yet.** `depth` over the overlay, locale selection (arrives with gap 2's `?locale=`).

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

- HTTP: `/content/v1/{project}/{env}/…` and `/preview/v1/{project}/{env}/…` (D-172).
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
- **In place since step 2.2:** `crates/colophon-server/tests/releases.rs` (anonymous reads, a newer draft never
  delivered, a pinned read of generation 1 after release 2, a cursor that keeps its generation across a release);
  `crates/colophon-db/tests/releases.rs` (the delivery role refused drafts, entries, changesets, members, sessions,
  identities and every write); `perf/k6/delivery.js` (anonymous latest, pinned and single-entry reads, at least
  1,000,000 per full run); `perf/k6/audience-sites.js` (the five demo sites' real read mix —
  `/media/v1` thumbs, renditions and originals beside collection lists with cursor paging,
  latest and pinned entries, `If-None-Match` revalidation and a made-up 404 — against the
  seeded `demo-sites` project, which is what found the media origin's per-request environment
  lookup D-134 memoises). DX.4: `crates/colophon-server/tests/delivery_query.rs` (filter, sort and page parity
  with the draft route, a pinned release unmoved by a newer one, unpublished and tombstoned entries absent at
  every generation, anonymous and credentialed reads, foreign and uncommitted-generation cursors refused,
  `fields` narrowing and the `applied` echo, ETag revalidation).

## Build order

1. **P2:** the public API from published projections without a cache; pinned reads (step 2.2, done).
2. **P3:** cache layer, CDN headers, tags, Cloudflare purge, probes, unpublish and revocation. Step 3.1 (D-048): the
   cache layer, CDN headers and tags; F06, F08 and L04 in `crates/colophon-server/tests/cache.rs`. Step 3.2 (in
   progress, D-049, D-050): unpublish, tombstones and origin revocation; purge jobs through a fake CDN.
3. **P4:** time-travel queries for the Studio slider, the preview origin, sitemaps, feeds and redirects.

## Open decisions

- **O-012:** probe locations for v0.1.
- How to implement single-flight coalescing across multiple API processes, if they are ever split.
