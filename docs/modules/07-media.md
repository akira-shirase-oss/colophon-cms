# 07 · media (`crates/colophon-media`)

> Uploads with their own lifecycle. No unchecked or unsafe file is ever published or served, and no referenced file is
> ever deleted.

| | |
|---|---|
| Milestones | P4 (images: intents, quarantine, verification, derivatives, references), P6 (retention and staged deletion) |
| Proposed owner | Claude |
| Depends on | domain (asset types and references), db (assets, references), worker (image pool), an object-storage crate, an image-decoding crate |
| Used by | api (`/assets`, `/upload-intents`), studio (media picker), delivery (asset origin), release validation |
| Spec | §18, §26, §27 |

## Why it exists

File uploads are one of the largest attack surfaces in any CMS. A browser-supplied MIME type proves nothing, and image
decoders can be pushed into huge allocations. Assets also have to live as long as any draft or retained release that
references them. A separate module with its own state machine keeps both concerns explicit.

## End goal

Every asset passes quarantine and verification before it can be used. Public and private storage are separate. An
entry can never publish an unprocessed asset, and garbage collection never removes an object that a draft or retained
release still references. Proven by L14, malicious-upload fixtures and X10.

## Responsibilities

- Upload intents and the quarantine step.
- Verification and derivatives.
- Reference tracking.
- Expiry, retention and staged deletion.
- The separate asset origin.

## Not its job

- Video transcoding (outside the product, spec §18).
- Private paid downloads (parked with commerce).
- Rendering images in pages, which belongs to the [content-renderer](12-content-renderer.md) and [web](15-web-blog.md).

## Breakdown

### Upload flow (spec §18)
1. Rust creates an upload intent with project scope, declared type, size and expiry.
2. The client uploads directly to a quarantine object key with a random storage name.
3. A worker job inspects the actual size and file signature and decodes the file with bounds on pixels, nesting and
   processing time.
4. Unnecessary metadata is stripped.
5. The asset is marked usable, and versioned derivatives (resized variants) are created.

### Allowed classes
- **v0.1:** approved images up to 20 MiB.
- **Later:** scanned PDF documents up to 50 MiB.
- **Parked with commerce:** private digital downloads up to 200 MiB.
- **Rejected by default:** executable HTML, archives and untrusted SVG.

### Storage and origin
- S3-compatible storage: Garage locally (D-016), a managed bucket in production. Only the S3 API is used, so the store can be swapped.
- Assets are served from a separate asset origin, never the administration origin.
- Public images and private objects use separate paths, visibility policies and access tests.
- Production and staging use separate buckets.
- The origin's only lookup — the URL's environment slug, the asset and an optional pinned
  version — is one `media_object` call: a SECURITY DEFINER function resolving the slug and the
  version's key in a single round trip (D-136's serving path subsumed the slug→id memo D-134
  added before versioning existed — a pinned version needs the database anyway). Everything
  else about the read stays as D-094 describes — no session, no asset-row read, object
  existence as the proof it verified.
- **Object keys are content-addressed** (D-136). Verification promotes the stripped bytes to `public/{project}/{environment}/{asset}/{sha256-hex}` — the version's digest is the object identity, so a retried landing writes the same object and a second landing can never overwrite the first. Renditions sit under the version's key (`…/{sha}/{w320}`). Assets verified before versioning keep their one `public/{project}/{environment}/{asset}` key, which is exactly the prefix the newer keys live under.
- `asset_versions` is the immutable landing record — `seq`, the upload's declared type and size, the verified type, dimensions, digest, literal `public_key` and rendition names — one row per distinct content, oldest first (`GET {P}/assets/{asset}` returns it as `versions`). Two landings of identical bytes coalesce on `(asset, sha256)`; a deleted asset keeps its history.
- Serving: `GET /media/v1/{project}/{environment}/{asset}` is the current version's original; `…/{asset}/{sha}` pins one version's original; `…/{asset}/{name}` is the current version's rendition; `…/{asset}/{sha}/{name}` a pinned version's. The single `media_object` read resolves all four — a usable asset's key, nothing for anything else, so quarantined, rejected, deleting and deleted rows 404 even while their objects are still in the store. Caching follows what the address names (D-157): a sha address names one version's bytes, which never change, so `…/{asset}/{sha}` and `…/{asset}/{sha}/{name}` answer `Cache-Control: public, max-age=31536000, immutable`; the bare `…/{asset}` and `…/{asset}/{name}` follow the current version, which a replace moves and a delete ends, so they answer `public, max-age=60, must-revalidate` with the version's `ETag`, and a matching `If-None-Match` gets `304` with no body.
- Replacing bytes: `POST {P}/assets/{asset}/replace` (`Idempotency-Key`, `asset.replace`, `ContentUpdate` project-wide) takes the same headers and body as an upload, writes a fresh quarantine object and queues a `reverify` job — `202` means queued, not swapped. Only `usable` accepts one: `asset_quarantined` while the first verdict is out, `asset_rejected` where there is no version to swap, `already_deleted` when it is gone. A refused replacement verdict leaves the current version serving; a landed one swings the asset's current-version columns in the same transaction that writes the version row.

### Processing limits
- Four image workers, each with process and time limits ([worker](05-worker.md) image pool).
- Image alt text is required by the blog schema for accessibility and SEO.

### References and lifecycle
- `asset_references` records every asset a revision names, exactly like `revision_references` records entries.
- An asset is collectable only when nothing names it: no live entry's `draft_revision_id` (`deleted_at IS NULL`), and **no `publications` row at all** — publications pin their assets forever, open or closed (D-098).
- `DELETE {P}/assets/{asset}` (`Idempotency-Key`, `asset.delete`, `ContentDelete` project-wide) refuses `quarantined` uploads (`asset_quarantined`), answers `already_deleted` for `deleting`/`deleted`, and `still_referenced` for anything still named; otherwise it commits a `delete` job plus an `asset.deleted` audit event — **202 means requested, not gone**.
- The worker's delete attempt locks the asset `FOR UPDATE`, rechecks the same keep-rule under the lock (the verdict that counts), marks `deleting`, supersedes the asset's not-yet-started verify/reverify jobs, then removes every object under `public/{project}/{environment}/{asset}` — the listing covers every version, every rendition and any stray, whatever layout each was written under — plus every quarantine key, outside any transaction, then lands `deleted`. Object removals are idempotent; a crash mid-list retries the job.
- The states: `quarantined` → `usable`/`rejected`, or → `deleting` → `deleted`. Asset rows are never removed; `deleted` rows still list and read, `ON DELETE RESTRICT` on the references is the backstop (D-098).
- The races close in the database, not in reads: `asset_references_asset_live` (migration 0039) takes `FOR KEY SHARE` on the named asset and refuses `deleting`/`deleted` assets and pending `delete`/`expire` jobs as `asset_not_found`; `publications_asset_live` takes the same lock on every asset a publication's revision names, so a publish can never commit against an in-flight delete. The same discipline covers a replace: a landing that meets `deleting` can't swing, the job ends `superseded`, and the worker removes the objects it just promoted — anything it can't reach is swept as an unrecorded `public/` object (below).
- **One lock order: the asset rows, then the account's counters** (D-249, migration 0099). Landing a verdict updates the asset (a key update: `public_key` is unique) and its statement trigger then moves the owner account's pending bytes in `account_usage`; deleting and expiring lock the asset `FOR UPDATE` first too. A save that names images takes `FOR KEY SHARE` on all of them, in id order, before it writes anything (`colophon_lock_assets`, a definer function reading the project from the row-security context), since creating an entry moves the same account's entry count. Before, a save took the counter first and the asset second, and a save naming an image could deadlock with that image's landing: the save answered 503, or the landing waited out its 30 s lease. A landing that loses a deadlock or serialization conflict anyway is recorded again at once (up to three times), not left to its lease.
- The retention sweep (D-099) runs beside the worker wherever a store is configured: every `COLOPHON_ASSET_SWEEP_MS` (default 300_000 — five minutes) it enqueues an `expire` job for every asset collectable for longer than `COLOPHON_ASSET_EXPIRY_MS` (default 86_400_000 — spec §18's 24 hours) — `quarantined`, `rejected` and `usable` alike, but in every state only while nothing names it: the delete request's keep-rule applied at sweep time, so even a draft naming a rejected upload still pins it (M3). A pass enqueues at most 200 jobs, oldest first over the `assets_collectable` index, and skips an asset already mid-job; what a pass leaves, the next collects.
- `expire` rides the delete attempt end to end — `mark_deleting` rechecks the same keep-rule under the row lock, so a name committed between the sweep's read and the claim fails the job, never loses an object; the only difference a kind makes is that `quarantined` holds a `delete` but not an `expire` — an abandoned quarantine is exactly what the sweep collects, missing object and all.
- Orphaned quarantine objects: the same pass lists at most 500 objects under `quarantine/` and removes each whose `last_modified` is older than the expiry and which no still-open work holds — a `quarantined` asset's own key or a pending/running verify or reverify job's — so a replacement's upload mid-flight is as safe as a first one, and leftovers of verified, rejected and deleting uploads go. An object younger than the expiry is never touched, which is what protects an upload still mid-verify.
- Orphaned public objects: the pass also lists `public/` and removes each object older than the expiry which no asset's current `public_key` and no `asset_versions` row serves from (D-136) — tombstones of a mid-swing crash or an orphaned promotion. A promoted object is recorded within the attempt deadline, far inside the expiry, so the age guard keeps a landing mid-write safe.
- What a release keeps: a publication pins the **asset** forever, and under content-addressed versions that is every version's bytes — a replace can never overwrite what a retained or previously published release was shown, and the pinned URL serves the version it saw. The bare URL always serves the *current* version, exactly as before.
- **Images inside rich text are references too** (D-177). Each distinct upload a rich-text value's `image` nodes name is one `asset_references` row of the field (`body`, `sections/2/body`), positioned in first-named order — at most 200 per document (migration 0066 widens positions to 0..199). A save that names an id which isn't an upload of the environment (a made-up id, another project's, the old Studio's random placeholder) is refused `422 asset_not_found` at that image's own pointer (`/body/doc/content/3/attrs/assetId`); the publish check (`asset_not_usable`, M1), the keep-rule (M3), the delete refusal, "Used in" and the 0039 race trigger then cover rich-text images with no new machinery. Revisions saved before 0066 carry no rich-text references until they are saved again.
- **Releases record the versions they publish** (D-178, closing this module's "stamp the version a release saw" follow-up). Each new publication's `media` holds, by asset id, the version current at release time — digest, verified type, dimensions, renditions (`colophon_release_media`, one indexed read per entry inside the release transaction); a rollback copies what the release it returns to recorded (`colophon_publication_media`). Delivery turns it into the versioned URL, renditions, size and `current_url` ([delivery](06-delivery.md)). A replace therefore reaches sites using the recorded URLs at the next publish: publishing an entry whose revision is already live reopens it when an upload it names has a newer version. Older publications were backfilled with the version current at migration time — an approximation.
- **The library** (D-179): `GET {P}/assets` pages newest first by an opaque keyset cursor on `(created_at, id)` (`limit` 1–100, `next_cursor`; the old list stopped silently at 100), and narrows by `q` (≤ 100 characters, a case-insensitive substring of the file name, title and alt text, matched against a generated `search` column. Row security keeps a non-leakproof `ILIKE` on `assets` from using an index, so a search reads its page of ids through `colophon_asset_search` (migration 0067): a definer function over `asset_search`, a trigger-kept copy of each asset's searchable text, state and tags with no row security and no grants, scoped to the transaction's `app.project_id` exactly as the `assets` policy is, served by a trigram index — 0.8–1.6 ms at 50,000 assets on the laptop, against 65–150 ms scanning; the rows then load through `assets` and its policy, and pages without `q` stay index-only under 1 ms), `state` and `tag`. Each asset carries the library's own words — a default `alt` (≤ 300) the editor's image dialog starts from, `title`, `credit` (≤ 200) and up to ten slug-shaped `tags` (tags, not folders) — changed by `PATCH {P}/assets/{asset}` under `If-Match` on `metadata_version` (`"v<n>"`, also the `ETag` of `GET`), with an `Idempotency-Key` (`asset.update`, `ContentUpdate`) and an `asset.updated` audit naming which fields changed, never their text. Image bytes stay public at `/media/v1` whatever the entry's audience; private media is parked.
- Deletion is staged and audited — and there is **no CDN purge path yet**. Since D-157 the bare addresses cache for a minute and then revalidate, so a delete or a replace reaches every cache within a minute; a version's sha address stays immutable for a year, so a *deleted* version's bytes may still linger at an edge that cached that address (D-098's accepted gap narrowed to pinned URLs; media purge comes with the retention milestone's later slice). Delivery now hands sites the versioned addresses a release recorded (D-178), which the reference and demo sites use.

## Invariants

| ID | Invariant |
|---|---|
| M1 | An entry cannot publish with a quarantined or unprocessed asset. |
| M2 | The client's declared MIME type is never trusted; actual signature and decode decide. |
| M3 | An object referenced by any draft or retained release is never garbage-collected. |
| M4 | Public and private objects never share a path or a visibility policy. |
| M5 | Uploads are never served from the administration origin. |

## Interfaces

- **API:** create an upload intent, complete verification, inspect (with its version history), list references, replace bytes, request deletion.
- **Worker jobs:** verify, reverify, expire, delete.
- **Release validation:** the asset-readiness check.

## Failure modes

| Situation | Behaviour |
|---|---|
| Decompression or pixel bomb | Bounded decode fails; the asset is rejected with a safe diagnostic |
| Misleading file type | Signature mismatch: rejected |
| Partial upload | The intent expires; no asset record becomes usable |
| Object store down | Editing continues; uploads fail clearly; publishing assets waits |
| Large media workload (L14) | Worker limits hold; content editing stays responsive |

## Tests and evidence

- L14 (large media and decoder workload).
- Malicious upload fixtures:
  - decompression and pixel bombs;
  - misleading file types;
  - HTML disguised as an image;
  - untrusted SVG.
- X10: 10,000 asset references across a template upgrade, with no referenced object deleted.
- A test that publishing with a quarantined asset is blocked.

## Build order

1. **P4:** intents, quarantine, verification, ~~the Studio media picker~~ (done, D-095), ~~derivatives~~ (done, D-096), ~~references~~ (done, D-094).
2. **P6:** ~~staged deletion: the `delete` request, job and tombstone states, the reference/publication race triggers~~ (done, D-098); ~~expiry, the orphan sweep, the Studio delete affordance, the testbed page and the k6 delete arm~~ (done, D-099); ~~byte versioning: content-addressed object keys, `asset_versions`, the replace endpoint and `reverify` job, pinned URLs, prefix-list deletion and the public-orphan sweep~~ (done, D-136). Still open: the CDN purge path the deleted bytes need (the D-098 gap: media responses carry no `Cache-Tag` yet). ~~Stamping a release's pinned version into the published projection~~ (done, D-178).
3. **Gap 3:** ~~rich-text images as references~~ (D-177), ~~release media and versioned delivery URLs~~ (D-178), ~~library paging, search, metadata~~ (D-179). ~~`media` for populated references~~ (D-178). ~~an index-served search~~ (migration 0067). Still open: media `Cache-Tag` and CDN purge on replace and delete, an operator audit of drafts still holding placeholder image ids, a `w640` rendition for a denser `srcset`, presigned uploads over 20 MiB, AVIF/GIF.

## Open decisions

- Image-decoding crate. It is not in the rust-mastery crate guide, so evaluate it with the chapter 15 checklist.
- `aws-sdk-s3` or `object_store` for storage (both are in the crate guide).
- Whether to decode in a separate process for isolation, not only in a separate pool.
- The exact v0.1 image format allowlist.
