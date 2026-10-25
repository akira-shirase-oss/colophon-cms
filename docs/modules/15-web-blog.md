# 15 · web and blog template (`apps/web`, `templates/blog`)

> The reference blog, which is also the maintainer's own site. It is the first real user of Colophon and the proof that
> delivery works end to end.

| | |
|---|---|
| Milestones | P3 (prerendered blog, generation marker, `/colophon` page, live deployment), P4 (feeds, sitemaps, language alternates) |
| Proposed owner | Claude builds; the maintainer owns the content and design taste |
| Depends on | sdk, content-renderer, ui; delivery (`/content/v1`); worker (build-and-promote adapter) |
| Used by | readers; the release pipeline's evidence probes |
| Spec | §05, §14, §18, §19, §25 (D-009 consolidates the template material from §18 and §19) |

## Why it exists

A CMS proves it is useful by running a real site. The maintainer's site on Colophon is the first and most honest
user. It is also the reference implementation of the hardest delivery path: a statically generated site, where a fresh
API response proves nothing about whether the pages were rebuilt.

## End goal

A fast, prerendered blog built from a release manifest pinned to one generation:
- every page carries a generation marker;
- builds are promoted atomically;
- a failed or stale build can never replace a newer one;
- it runs in production as the maintainer's site, with a `/colophon` page showing the live release and its receipts.

## Responsibilities

- The template manifest and its install and upgrade.
- Blog schemas and pages.
- The pinned build and the generation marker.
- The build-verify-promote adapter.
- The `/colophon` page.

## Not its job

- Business sites, commerce, membership or knowledge-base templates ([parked](parked.md)).
- Server-side CMS authority: the site reads published content only.

## Breakdown

### Template manifest (spec §18, §19)
- **Contents:**
  - version and compatible core range;
  - schema manifest and renderer version;
  - migrations and requested permissions;
  - fixture data and tests;
  - asset licences.
- **Installation** previews the planned changes and is idempotent.
- **Upgrades** preserve local edits or present explicit conflicts, and never overwrite customized content or components
  silently.
- **Fixtures** contain no invented customers or testimonial claims (spec §04).

### Blog content model (spec §05)
- Posts, authors and categories.
- SEO fields: title, slug, description, canonical URL override, image alt text, social metadata.
- Images, through [media](07-media.md).

### Pages
- Home, post list, article, author, category and 404.
- RSS/Atom feed, sitemap and language alternates, all generated from one published generation.
- The **`/colophon` page**: the live release generation, when it was published, and its delivery receipts. The project's
  name, used as a feature.

### Pinned build and promotion (spec §19, §25)
- Each build reads one release manifest pinned to `(epoch, generation)`.
- Every page includes a machine-readable generation marker.
- Each deployment attempt records:
  - the source commit and the schema and renderer hashes;
  - the CMS generation;
  - the provider deployment ID and the serving URL.
- The flow is **build → verify output → atomically promote**. After promotion, a probe checks the deployed marker.
- A build failure keeps the previous site live, and the Studio shows *Website update pending*.
- Promotion is serialized per destination. A slow older build may finish, but it can never promote a generation older
  than the currently desired one (X08).

## Invariants

| ID | Invariant |
|---|---|
| B1 | Every page belongs to exactly one generation, shown by its marker. |
| B2 | An older build never becomes current without a new authorized rollback. |
| B3 | A failed build never takes the live site down. |
| B4 | Template upgrades never silently overwrite customized content or components. |

## Interfaces

- Reads `/content/v1` through the [sdk](13-sdk.md) with pinned reads.
- Exposes its generation marker to probes in [delivery](06-delivery.md).
- Uses the site adapter run by [worker](05-worker.md).

## Failure modes

| Situation | Behaviour |
|---|---|
| Build fails | Previous site stays live; *update pending*; error visible in receipts |
| Slow old build finishes after a newer release (X08) | Not promoted; recorded as superseded |
| Host deployment response lost | Reconcile with the provider deployment ID before retrying |
| Content API unavailable during build | Build fails cleanly; nothing is promoted |

## Tests and evidence

- A Playwright journey across the published site.
- X08 (slow older build).
- A marker check after promotion.
- Lighthouse-style performance and accessibility budgets.
- Template install and upgrade tests with customized content.

## Build order

1. **P3:** schemas, pages, pinned build, marker, `/colophon` page, build-verify-promote, live deployment.
2. **P4:** feeds, sitemaps, language alternates, slug redirects.

## Open decisions

- **O-012:** hosting for the reference site and probe locations.
- Whether the site is SvelteKit-prerendered from day one or starts simpler and moves to SvelteKit.
