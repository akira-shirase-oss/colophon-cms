# 16 · docs site (`apps/docs`)

> Guides, references, runbooks and the technical write-ups. It is honest about what is implemented and what is only
> planned.

| | |
|---|---|
| Milestones | P3 (skeleton and quickstart), P6 (complete guides; every example runs in CI) |
| Proposed owner | Claude; the maintainer writes the deep-dive articles |
| Depends on | ui; mdsvex, Pagefind |
| Used by | editors, developers, operators, contributors, portfolio readers |
| Spec | §18, §19, §27 |

## Why it exists

An independent install from the README is part of v0.1's definition of done, and documentation that drifts from the
code is worse than none. The deep-dive write-ups are also half of the project's portfolio value.

## End goal

A prerendered, searchable docs site. Every code example runs in CI, links are checked on release, and every page
clearly labels what is implemented vs planned.

## Responsibilities

- Guides, references, runbooks and write-ups.
- Static search.
- Example testing.
- Capability labels.

## Not its job

- Product marketing claims. There are no uncited speed claims and no "secure by default" promise (spec §18).

## Breakdown

- **Stack:** mdsvex + Pagefind, prerendered, using the same design system as the site with its own navigation.
- **Content:**
  - editor onboarding and a developer quickstart;
  - schema examples;
  - CLI, API and SDK reference;
  - self-hosting, backup and restore, upgrades;
  - troubleshooting, security reporting, known limits.
- **Deep dives:**
  - the outbox;
  - release generations and epochs;
  - the deterministic simulator;
  - cache invalidation;
  - delivery receipts.
- **Versioning:** product docs are versioned (spec §19).
- **CI:** code examples and template installs run in CI, and links are checked.

## Invariants

| ID | Invariant |
|---|---|
| DS1 | No page describes a planned feature as available. |
| DS2 | Every code example on a released version runs in CI. |

## Interfaces

- Static site output, plus a Pagefind index.

## Failure modes

| Situation | Behaviour |
|---|---|
| An example breaks after a change | CI fails before the release |
| A link goes dead | The release link check fails |

## Tests and evidence

- Example execution and link checks in CI.
- The independent-install trial uses only the docs, and its friction is written up.

## Build order

1. **P3:** skeleton and quickstart.
2. **P6:** complete guides, references, runbooks and the deep dives.

## Built so far

On branch `site/docs` (not yet merged), rebuilt on 2026-10-27 in the website's look:

- **`apps/docs`** (`@colophon/docs`): SvelteKit with the static adapter, every page prerendered to `<address>/index.html`
  (trailing slashes), TypeScript strict, Markdown pages through mdsvex 0.12.8. Built for the base path `/docs`
  (`COLOPHON_DOCS_BASE`), because the website links to `/docs/` on its own host (module 20); links written in a page
  as `/concepts/…` get the base path at build time, and `site:/roadmap/` becomes a link to the website. Dev server on
  `127.0.0.1:58310`; `npm run preview -w @colophon/docs` serves `build/` under `/docs/` on `127.0.0.1:58311`
  (`scripts/serve.js`; `--with-site` also serves the website's build at `/`, as one host would).
- **The website's look, shared, not copied:** `packages/site-theme` (`@colophon/site-theme`) holds the tokens (light,
  and the warm brown dark theme the maintainer chose), Inter and Fragment Mono, the type scale, pills, chips, glass and
  the theme switch's transition; both `apps/site/src/app.css` and `apps/docs/src/app.css` import it right after the
  design system. The docs' header is the website's glass pill (mark, "Docs", the sections as tabs, search, the website,
  GitHub, the one-button light/dark switch), the footer is the website's card, and every page carries a "Pre-release,
  not for production" banner. The theme switch, the mark and the brand icons are the website's files; a test fails if
  they, the shared imports or the status words drift.
- **Information architecture** (`src/lib/nav.ts`, the one site map): Get started (what Colophon is, run it locally,
  the first project, article and publish with its receipt), For editors (in D-059's plain words), Concepts (fourteen
  pages, scheduled publishing and onboarding among them), Developer reference (API conventions,
  all routes, one page per area with every route's who, headers, bodies and errors, the error envelope with every
  code and reason, the delivery API, the CLI, rich text, webhooks), Operations (requirements, the local stack, every
  setting, health checks, the worker, every migration, backups, crash tests), Resources (security model, performance,
  roadmap and status, FAQ, glossary) and Updates (the changelog, release notes, upgrade guides), then the design
  documents.
- **Statuses in the website's words (DS1):** every page carries *Done · step*, *Building · step or branch* or
  *Planned · milestone*, from the site map, plus *Overview* for pages that introduce others and *Design document*
  for the rendered design documents. Parts of a page that differ carry an inline label (`<Status planned="R.4" />`).
  Pages describe the code they ship with, the checked-out tree: since 2026-10-27 that is main plus 3.6, 3.7, R.1 to
  R.3 and scheduled publishing, merged together, and only the docs site and the website are still Building.
- **Changelog and release notes** (docs/RELEASES.md): `/docs/changelog/` renders `CHANGELOG.md` (Keep a Changelog
  groups inside each step, an anchor per milestone and per step) with an Atom feed of merged steps at
  `/docs/changelog/feed.xml`; the website renders the same file at `/changelog/`. Release notes per milestone
  (`docs/releases/p0.md` …, from `docs/templates/release-notes.md`) render at `/docs/releases/…`, their "All changes"
  replaced at build time by the milestone's section of the changelog; upgrade guides per range of migrations
  (`docs/upgrading/*.md`, from the template) at `/docs/upgrading/…`: one per milestone up to P3, and
  `p3-roles-schedules.md` for 0016 to 0022, which arrived together. Milestones R and E have changelog sections but no
  release notes yet.
- **Rendered from the files:** the design documents, the release notes and the upgrade guides are read and rendered
  at build time with unified (remark-gfm), so there is no copy to drift. The performance page reads every
  `perf/results/*.json`, with its machine, date, commit and threshold outcome, misses included.
- **Search:** Pagefind 1.5.2 indexes every built page, the changelog included, after `vite build`; the site's own
  dialog (⌘K, Ctrl+K or /) loads its index from the same origin and answers long pages with the heading that
  matched (a changelog entry, a route). Nothing loads from another origin.
- **Tests** (`npm run test -w @colophon/docs`, Playwright, after a build): every page renders with no console errors
  or warnings, one h1, a status label, the banner and no request to another origin; a crawl from the front page finds
  every page, every link and `#fragment` resolves, and every link to the website names one of its pages, and every
  docs address the website links to exists; axe (WCAG 2.1 AA) passes on every page in light and in dark, and in the
  search dialog and the phone navigation; every page reflows at 320 px; the theme switch is one button and survives a
  reload; the shared look; search finds pages, a changelog entry and the design documents; the changelog page lists
  every section and entry of `CHANGELOG.md` with their anchors, the feed every merged step, and each release note its
  milestone's section; every documented route exists in the router and every route there is documented once, never
  as Planned; the errors page lists every code and every reason the code can put in `details` (from every module of
  the API crate and every `code()` table in the domain); the code is read from the checked-out tree, so the checks
  work on CI's shallow clone, and `COLOPHON_DOCS_CODE_REF` checks the docs against another commit with `git show`; every runnable command exists; the CLI page lists every command.

Not done yet: versioned docs (spec §19), executing the examples in CI (today they are checked to exist, not run),
deep-dive write-ups, troubleshooting and security-reporting pages, and hosting.

## Open decisions

- Where docs are hosted, and whether the docs site is itself published through Colophon later.
- Whether pages that only introduce others should carry an *Overview* label (as built) or one of the three
  capability labels.
