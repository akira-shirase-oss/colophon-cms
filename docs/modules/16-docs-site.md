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

The P3 skeleton, on branch `site/docs` (not yet merged):

- **`apps/docs`** (`@colophon/docs`): SvelteKit with the static adapter, every page prerendered to `<address>/index.html`
  (trailing slashes, so any static host serves it as it is), TypeScript strict, Markdown pages through mdsvex 0.12.8,
  and the design system as is (`@colophon/ui/styles.css`, its components, mode-watcher for light and dark). Dev
  server on `127.0.0.1:58310`; `npm run preview -w @colophon/docs` serves `build/` on `127.0.0.1:58311` with a small
  static server (`scripts/serve.js`), because Vite's preview doesn't serve the Pagefind index written after the build.
- **Information architecture** (`src/lib/nav.ts`, the one site map): Get started (what Colophon is, a developer
  quickstart, status and roadmap), For editors (seven guides in D-059's plain words, with a table mapping them to
  the Studio's current words), Concepts (eleven pages, from revisions to the simulator), Reference (API conventions,
  every route, the error envelope and codes, the delivery API, the CLI, the rich-text format, webhooks), Operations
  (the local stack, configuration, load testing, crash tests, and a planned page for backups and upgrades).
- **Capability labels (DS1):** every page carries one, from the site map, never from the page: *Implemented*,
  *Building · step*, *Planned · milestone*, plus *Overview* for pages that introduce others and *Design document*
  for the rendered design documents, which mix built and planned. Parts of a page that differ carry an inline label
  (`<Status planned="P4" />`). A Markdown page missing from the site map fails the build.
- **Design documents rendered from the files:** `docs/*.md`, `docs/modules/*.md`, `docs/formats/*.md` and
  `docs/brand/README.md` are read and rendered at build time with unified (remark-gfm), so there is no copy to
  drift. Links between them stay on the site, other repository links go to GitHub, and heading ids match GitHub's.
- **Load results from the files:** the load-testing page reads every `perf/results/*.json` at build time and shows
  each run with its machine, commit and k6 threshold outcome, misses included. The site has no other performance
  figures.
- **Search:** Pagefind 1.5.2 (MIT) indexes the built pages after `vite build`; the site's own dialog (⌘K, Ctrl+K or
  /) loads its index from the same origin. Nothing loads from another origin: fonts, styles, scripts and the index
  are all served by the site.
- **Code blocks:** highlighted at build time by Shiki with a CSS-variables theme mapped onto the design tokens, so
  every code colour is a token the contrast test covers; each has a copy button. Blocks marked `runnable` are
  checked: every command in them must exist in the repository.
- **Tests** (`npm run test -w @colophon/docs`, Playwright; it builds the site first, so `scripts/frontend.sh`'s
  `npm test --workspaces` runs them too): every built page renders with no console
  errors or warnings, one h1, a capability label and no request to another origin; a crawl from the front page
  finds every page and every link and `#fragment` resolves; axe (WCAG 2.1 AA) passes in light and dark on a sample
  of each section, the search dialog and the phone navigation sheet; pages reflow at 320 px without sideways
  scrolling; search finds known pages, including the design documents; every runnable command exists (scripts and
  their subcommands, npm scripts, crates, features, test files, CLI subcommands and flags, simulator flags); and the
  references list every error code, CLI command and route the code defines.

Not done yet: versioned docs (spec §19), executing the examples in CI (today they are checked to exist, not run),
deep-dive write-ups, troubleshooting and security-reporting pages, and hosting.

## Open decisions

- Where docs are hosted, and whether the docs site is itself published through Colophon later.
- Whether pages that only introduce others should carry an *Overview* label (as built) or one of the three
  capability labels.
