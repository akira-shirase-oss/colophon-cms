# 20 · public website (`apps/site`)

> Colophon's own website: what it is, what is done and what is only planned, the comparison with Strapi and Sanity, the
> roadmap, the changelog and the build log. Everything on it comes from the repository's files, and every feature
> carries its status.

| | |
|---|---|
| Milestones | P3 (first version, alongside the docs skeleton); kept current at every step |
| Proposed owner | Claude builds; the maintainer owns the words and the taste |
| Depends on | [ui](14-ui.md) (tokens and components, as they are), SvelteKit with the static adapter, markdown-it and smol-toml at build time only; reads `CHANGELOG.md`, `perf/results/*.json` and the build log's posts |
| Used by | people deciding whether to look closer: editors, developers, contributors, portfolio readers |
| Spec | §18 (no uncited claims), §35 (open-source hygiene) |

## Why it exists

A project whose identity is proof needs a front door that keeps the same rules: no number without its evidence, no
feature without its status, no claim the repository can't back. The README carries too much for a first visit, and the
build log tells the story but not the state. The site gathers both, and the docs site ([16](16-docs-site.md)) teaches.

## End goal

A prerendered, self-hosted site whose every page is checked in CI: accessible in light and dark, nothing loaded from
another origin, and content that can't drift from its sources because it is read from them at build time.

## Responsibilities

- Pages: home, product, compare, roadmap, changelog, blog (index and one page per post), open source, 404.
- The changelog page, from `CHANGELOG.md` at the repository root.
- The blog, from the build log's posts copied unchanged into `apps/site/src/content/blog`.
- Load figures, from `perf/results/*.json`.
- `/blog/rss.xml`, `/sitemap.xml` and `/robots.txt`, prerendered.

## Not its job

- Teaching or reference material: that is the docs site, served under `/docs/` (linked, never built here).
- The maintainer's own site, which is the reference blog ([15](15-web-blog.md)) running on Colophon.
- Analytics, sign-up forms, newsletters or anything that runs on a server.

## Breakdown

### Content and its sources

| Page | Source of truth | How it stays honest |
|---|---|---|
| Changelog | `CHANGELOG.md`, parsed at build time (`src/lib/server/changelog.ts`) | One entry per step merged into `main` (`git log --merges main`), written from the step's commits and decisions; in-progress steps under *Unreleased* say they aren't merged; a malformed entry fails the build |
| Blog | `src/content/blog/*.md`, copied byte for byte from `minimal-portfolio/content/colophon` by `scripts/import-blog.ts` | Rendered like Zola renders it (CommonMark, smart punctuation, raw HTML escaped); each post's canonical link is its original at `https://404human.dev/colophon/<slug>/`, the slug made by Zola's rule |
| Load figures | `perf/results/*.json`, globbed at build time (`src/lib/server/perf.ts`) | Each figure names its file, date, commit and machine (`apple-m2-pro-12c`), says what its run measured, and carries the laptop caveat; a missing file fails the build. The regression verdict isn't in the files, so it is taken from the commit that added each one |
| Product, compare, roadmap | `src/lib/data/*.ts`, transcribed from `docs/COMPARISON.md` and `docs/ROADMAP.md` (2026-10-27) | Every feature is *done* (merged, with its proof), *building* (a step in progress) or *planned* (with its milestone); the comparison keeps its evidence notes, and says where it follows D-057 over older cells |

### Look

The design system as it is (D-056): warm paper, white cards on `shadow-soft` and `shadow-float`, black pill primary
buttons, outline pills, the burnt-orange accent only as `.glow`, `.haze` and `text-brand-ink`, the display classes and
the state badges (green done, amber building, grey planned, always with a word). One site-local size, `.display-hero`,
is the display cut one step larger. Product pictures are HTML compositions of the Studio's real states and words (a
receipt *Verified at 1 location*, the autosaving editor, an approved publish request, a roles matrix marked
*Planned · R*), labelled as illustrations. Layouts take their patterns from SaaS site templates (a hero over a product
visual, a bento of promises, a numbered how-it-works row, figure cards, comparison tables, a changelog timeline, a
milestone-grouped post index, a sitemap footer), not their text or images.

### Build

SvelteKit with `@sveltejs/adapter-static`: every route prerendered with trailing slashes; `404.html` is the fallback a
static host serves. Server-only modules (`src/lib/server`) run only while prerendering, so markdown-it and smol-toml
never reach the browser. Absolute URLs (sitemap, feed, robots) use `COLOPHON_SITE_ORIGIN`, default
`https://colophon.invalid` until hosting is decided. Links to `/docs/` carry `rel="external"` so the prerenderer
doesn't try to build another app's pages; any other broken internal link or missing anchor fails the build.

## Invariants

| ID | Invariant |
|---|---|
| W1 | No number appears that isn't in a committed result file, with its file, machine and the laptop caveat. |
| W2 | Every feature shows exactly one of done, building or planned; unsure means planned or left out. |
| W3 | No testimonials, customer logos, user, star or download counts, ratings or team bios. |
| W4 | Nothing is requested from another origin: no fonts, scripts, images, analytics or embeds. |
| W5 | The changelog and the blog are rendered from their sources at build time; they are never edited on the site. |
| W6 | Every page passes axe (WCAG 2.1 AA) in light and dark and fits a 320px screen. |
| W7 | The licence is described as proposed, not confirmed, until O-014 closes. |

## Interfaces

- `npm run dev -w @colophon/site` (127.0.0.1:58300), `npm run build`, `npm run preview` (58301).
- `npm run import-blog -w @colophon/site [source]`, or `COLOPHON_BUILD_LOG_DIR`, to refresh the posts.
- `CHANGELOG.md`'s shape: `## <section>`, `### <step> · <title>`, then `- **Merged:** <date> · <commits>` (or
  `- **Status:**` and `- **Branch:**` while unmerged), `- **Decisions:**`, `- **Build log:**`, then the body.

## Failure modes

| Situation | Behaviour |
|---|---|
| A cited result file is renamed or deleted | The build fails and names the file |
| A changelog entry lacks its step, title, date or commit | The build fails and names the entry |
| A post lacks a title or a date | The import script and the build fail |
| A post is added to the build log but not imported | The blog test fails when the source sits next to the checkout; CI, which has no source, checks the copies only |
| An internal link or anchor breaks | Prerendering fails |
| The docs aren't deployed at `/docs/` | Those links 404 on the host; nothing here can detect it |

## Tests and evidence

`apps/site/tests/site.spec.ts` (Playwright, Chromium, against the built site): every page with one h1, no console
errors and no axe violations in light and dark; no request to another origin across every page and a client-side
navigation; one blog entry per post file, canonical links, byte-for-byte copies; changelog entries equal to
`CHANGELOG.md`'s, in order; feed and sitemap contents; statuses on every feature; the licence's wording; the appearance
menu; the phone menu with axe; no sideways scrolling at 320px. `tests/screenshots.spec.ts` saves every page at 1440 and
390 wide, light and dark, when `SITE_SHOTS_DIR` is set. The site serves no requests of its own, so D-018's testbed page
and load scenario don't apply.

## Build order

1. **P3:** the first version (this document).
2. **Every step:** a new `CHANGELOG.md` entry, re-imported posts, statuses moved in `src/lib/data`.
3. **Before 3.8:** a host and an origin (O-012), and the docs at `/docs/`.

## Open decisions

- Hosting and the public origin (with O-012).
- Whether `src/lib/data` should be generated from `docs/COMPARISON.md` and `docs/ROADMAP.md` instead of transcribed,
  once their tables are stable.
