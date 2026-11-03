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

### Processing limits
- Four image workers, each with process and time limits ([worker](05-worker.md) image pool).
- Image alt text is required by the blog schema for accessibility and SEO.

### References and lifecycle
- `asset_references` records every asset a revision names, exactly like `revision_references` records entries.
- An asset is collectable only when nothing names it: no live entry's `draft_revision_id` (`deleted_at IS NULL`), and **no `publications` row at all** — publications pin their assets forever, open or closed (D-098).
- `DELETE {P}/assets/{asset}` (`Idempotency-Key`, `asset.delete`, `ContentDelete` project-wide) refuses `quarantined` uploads (`asset_quarantined`), answers `already_deleted` for `deleting`/`deleted`, and `still_referenced` for anything still named; otherwise it commits a `delete` job plus an `asset.deleted` audit event — **202 means requested, not gone**.
- The worker's delete attempt locks the asset `FOR UPDATE`, rechecks the same keep-rule under the lock (the verdict that counts), marks `deleting`, removes every object it ever wrote — public key, each rendition under it, the quarantine object — outside any transaction, then lands `deleted`. Object removals are idempotent; a crash mid-list retries the job.
- The states: `quarantined` → `usable`/`rejected`, or → `deleting` → `deleted`. Asset rows are never removed; `deleted` rows still list and read, `ON DELETE RESTRICT` on the references is the backstop (D-098).
- The races close in the database, not in reads: `asset_references_asset_live` (migration 0039) takes `FOR KEY SHARE` on the named asset and refuses `deleting`/`deleted` assets and pending `delete`/`expire` jobs as `asset_not_found`; `publications_asset_live` takes the same lock on every asset a publication's revision names, so a publish can never commit against an in-flight delete.
- An unreferenced upload can expire after 24 hours (retention step 2; the `expire` job kind and the `assets_collectable` index already exist for it).
- Deletion is staged and audited — and there is **no CDN purge path yet**: the origin answers `Cache-Control: public, max-age=31536000, immutable`, so a deleted asset's bytes may linger in CDN edges and browser caches past deletion (D-098's accepted gap; media purge comes with the retention milestone's later slice).

## Invariants

| ID | Invariant |
|---|---|
| M1 | An entry cannot publish with a quarantined or unprocessed asset. |
| M2 | The client's declared MIME type is never trusted; actual signature and decode decide. |
| M3 | An object referenced by any draft or retained release is never garbage-collected. |
| M4 | Public and private objects never share a path or a visibility policy. |
| M5 | Uploads are never served from the administration origin. |

## Interfaces

- **API:** create an upload intent, complete verification, inspect, list references, request deletion.
- **Worker jobs:** verify, derive, expire, delete.
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
2. **P6:** ~~staged deletion: the `delete` request, job and tombstone states, the reference/publication race triggers~~ (backend done, D-098); expiry and the orphan sweep next, then the Studio delete affordance, the testbed page and the k6 arm.

## Open decisions

- Image-decoding crate. It is not in the rust-mastery crate guide, so evaluate it with the chapter 15 checklist.
- `aws-sdk-s3` or `object_store` for storage (both are in the crate guide).
- Whether to decode in a separate process for isolation, not only in a separate pool.
- The exact v0.1 image format allowlist.
