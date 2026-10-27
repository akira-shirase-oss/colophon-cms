# 20 · public website (`apps/site`)

> Colophon's own website: what it is, what is done and what is only planned, the comparison with Strapi and Sanity, the
> roadmap, the changelog and the build log. Everything on it comes from the repository's files, and every feature
> carries its status.

| | |
|---|---|
| Milestones | P3 (first version, alongside the docs skeleton); kept current at every step |
| Proposed owner | Claude builds; the maintainer owns the words and the taste |
| Depends on | [ui](14-ui.md) (components, with the site's own token values over them), SvelteKit with the static adapter, Inter and Fragment Mono from `@fontsource` (self-hosted), markdown-it and smol-toml at build time only; reads `CHANGELOG.md`, `perf/results/*.json`, `src/lib/data/roadmap.ts` and the build log's posts |
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

- Pages: home, product, compare, roadmap, changelog, blog (index and one page per post), open source, 404, all on one
  layout measured from a reference (see *Look*).
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

On 2026-10-27 the maintainer asked for the site to be rebuilt on a reference layout, "pixel to pixel": the Dreelio
template's home, blog index and post pages (a commercial Framer template, dreelio.framer.website). Its layout,
measurements and motion are reproduced in the site's own code; none of its text, images, mockups, icons, logo,
people or font files are used. The Studio keeps D-056's look; this one is the site's alone.

**Where the numbers came from.** Playwright loaded the reference at 1,440, 1,200, 810 and 390px, scrolled it so its
entrances ran, and read the computed styles and boxes of every text and surface, the inline transforms of its
animations frame by frame, and its hover states. The condensed result (breakpoints, measure, type scale, colours,
radii, shadows, bar, buttons, each section's geometry, the motion curves) is the spec the code follows, and a script
measures the same elements on both pages at all four widths. At 1,440 the headline (76px, top at 160), the hero
(1,020px), the showcase (1,072 by 700), the feature rows (588 by 695 pictures, 202 by 48 tags), the bento (524 by 559
and 341 by 304 cells), the quote (64/92.8px), the pricing cards (498, and 578 against 581) and the footer card (1,072
by 399 against 395) match to within a few pixels, and the hero window's transformed box to within one. What differs
is content: Colophon's words wrap differently, and there are three community cards and three footer columns.

**Tokens** (`packages/site-theme/src/theme.css`, the one token source the site and the docs share; `src/app.css`
imports it after the design system and adds only the site's page grounds, sections, article column, bento and motion):
the design system's token names with the reference's values, light and dark. Ink `#1a1615`, copy `#453f3d`, captions `#66615f` (a step darker than the
reference's `#757170`, which fails AA on the bluest part of the page), eyebrows `#614a44`; a page ground that runs from
paper through sand to sky; a sky-to-sand band behind every page's first screen; glass cards (white at 70%, blurred)
and sand cards (`#f0eae5`), 24px corners, shadows only on icons and frames. Post labels use the reference's hues,
deepened until white text passes AA. The milestone under way is a flat card like its neighbours, marked only by its
outline. Dark keeps to warm browns, at the maintainer's request: a warm near-black page and band, dark-brown cards,
white pills, and the orange accent (not blue) for the current milestone's outline and the focus ring; no blue except
what means something. Breakpoints are the reference's: `tb` from 810px, `dt` from 1,200px. The measure is 1,072px,
with 40px gutters on a tablet and 16 on a phone; sections are 160px apart on a desktop, 96 on a tablet, 80 on a phone;
a heading sits 56px above its content on a desktop and 40 below that.

**Type.** Inter in its text cut with its default letter forms: it matches the reference's measured widths within 1%
(the reference's computed family is Open Runde, a rounded derivative of Inter with the same metrics). Fragment Mono
for code. Headings are 600 with -3% tracking: 76, 58 and 40px for the h1 at the three layouts, 52, 44 and 28 for an
h2, then 40, 32, 28 and 20px; copy is 20/30 and 18/27; eyebrows are 15px uppercase; buttons are 55px pills (18 by 24px
of padding) whose label rolls up on hover.

**The home page**, in the reference's order: the hero (headline, lead, two pills, the Studio's window in a frame); a
strip of the open-source projects Colophon is built on; the showcase (the Studio on a desktop, the reference site on a
phone, as tabs); two feature rows (writing, publishing) with true tags and panels of the Studio's real states; the
bento of guarantees (two cells with live graphics, three with icons, each with its status); the quote; the pricing
cards; the four newest posts; where to follow along; the footer. Its content keeps the site's rules:

| Reference section | Here | Why |
|---|---|---|
| Logo marquee of customers | Text wordmarks of the projects in the stack | W3: no customer logos |
| Testimonial and review cards | The question in `docs/WHY.md`; sentences copied from build-log posts, each linked to its post | W3, W9 |
| Three plans, a billing switch, "Save 20%" | Done, under way and planned, from `src/lib/data/roadmap.ts`; the switch shows the milestone now or next; a line says there are no prices | There is nothing to buy and no hosted version |
| Follower counts | Each place's address | W3 |
| Photographs and clouds | HTML illustrations of the Studio and the site; step-number covers for posts; clouds drawn with gradients | Not ours to copy |

The flow diagram, the API panes and the load figures have no place in that layout, so they are on the product page.
The blog index is the reference's: the newest post as a wide card in the band, then every post as a card, grouped by
milestone. A post is a centred title, lead and byline over its cover, the post in a 660px column, and three more posts.

### Motion and live graphics

The reference's motion, sampled from its frames, rebuilt with CSS and `src/lib/motion.ts` (no animation library),
moving only transform, opacity and filter:

- **Headline:** each word rises 10px out of a 10px blur over 1.2s (an ease-out quart), 50ms after the one before; the
  lead follows 100ms behind; the buttons fade in over 650ms from 740ms. Pure CSS, declared only under `html.js` with
  motion allowed.
- **The hero's window** lies back (`perspective(1000px) translateY(-80px) scale(0.8) rotateX(20deg)`) while its top
  is at the viewport's foot and stands up as the page scrolls its own height further (`scrollLinked`); on load it
  eases from lying back to where the scroll puts it. Inside, the Studio plays out a publish: typing, the save, the
  approved publish request, the receipt checking off to *Verified at 1 location*.
- **Sections** rise 64px and fade in once, on a curve sampled from the reference (34% at 13%, 80% at 34%, 94% at
  51% of a second; `reveal`). Only something below the fold is ever armed, so nothing on screen blinks out.
- **Feature panels** rise 60px into place as their row scrolls through the screen.
- **The strip** of projects drifts left at 50px a second with faded edges, only while on screen.
- **The bar** is clear over the band; once the page scrolls, a glass pill fades in behind it and its ends draw in.
- **Hover:** pill labels roll up one line (260ms); primary pills go to 85% ink; nav links get a white wash; covers
  grow 14.5% over 600ms; footer links darken over 400ms.

Bento graphics run only while in view (`data-run`: armed, on, off). The prerendered HTML, readers without JavaScript
and readers who ask for reduced motion get every final frame: words in place, the window flat, sections where they
belong, the strip still. Decorative pictures are `aria-hidden` and carry a sentence instead.

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
| W8 | Under reduced motion nothing animates and every graphic shows its final frame; a graphic shows only real behaviour or real numbers, with its status. |
| W9 | A quote on the site is copied word for word from a file in the repository or a build-log post and names its source; the build fails if it drifts. |

## Interfaces

- `npm run dev -w @colophon/site` (127.0.0.1:58300), `npm run build`, `npm run preview` (58301).
- `npm run import-blog -w @colophon/site [source]`, or `COLOPHON_BUILD_LOG_DIR`, to refresh the posts.
- `CHANGELOG.md`'s shape: `## <section>`, `### <step> · <title>`, then `- **Merged:** <date> · <commits>` (or
  `- **Status:**` and `- **Branch:**` while unmerged), `- **Decisions:**`, `- **Build log:**`, optionally
  `- **Proof:**`, then the changes under Keep a Changelog's groups (`#### Added`, `#### Changed`, `#### Fixed`…).
  Anchors are `#step-<step>`; a step that appears twice (`Fix`) adds its title. The docs render the same file at
  `/docs/changelog/` with an Atom feed (`/docs/changelog/feed.xml`), which the changelog page links to.
- Links into the docs (`DOCS`, `QUICKSTART`, `DOCS_CHANGELOG`, `CHANGELOG_FEED` in `src/lib/site.ts`): the docs'
  tests fail if one isn't a docs page.

## Failure modes

| Situation | Behaviour |
|---|---|
| A cited result file is renamed or deleted | The build fails and names the file |
| A changelog entry lacks its step, title, date or commit | The build fails and names the entry |
| A post lacks a title or a date | The import script and the build fail |
| A post is added to the build log but not imported | The blog test fails when the source sits next to the checkout; CI, which has no source, checks the copies only |
| An internal link or anchor breaks | Prerendering fails |
| The docs aren't deployed at `/docs/` | Those links 404 on the host; the docs are built for `/docs` and their tests check every address the site links to |

## Tests and evidence

`apps/site/tests/site.spec.ts` (Playwright, Chromium, against the built site): every page with one h1, no console
errors and no axe violations in light and dark; no request to another origin across every page and a client-side
navigation; one blog entry per post file, canonical links, byte-for-byte copies; changelog entries equal to
`CHANGELOG.md`'s, in order; feed and sitemap contents; statuses on every feature; the licence's wording; the appearance
menu; the phone menu with axe; no sideways scrolling at 320px. `apps/site/tests/motion.spec.ts`: under reduced motion no
graphic switches on and nothing animates, and the hero shows its last frame; with motion the story plays, rests and
replays, axe passes in light and dark while graphics run, loops pause off screen, every bento cell has a status and
every picture its words, and the slider, API tabs and backlog filter work from the keyboard.
`apps/site/tests/look.spec.ts`: the home page's sections in the reference's order; its key measurements at 1,440px;
quote cards that link to the posts they were copied from; Inter and Fragment Mono, self-hosted; the showcase's tabs
and the milestones' switch from the keyboard; with motion, the headline word by word, the window standing up as the
page scrolls, a section rising in once and resting, the bar turning into a pill and the strip drifting; under reduced
motion and without JavaScript, final frames only. `tests/screenshots.spec.ts` saves every page at 1440 and 390 wide,
light and dark, when `SITE_SHOTS_DIR` is set. The site serves no requests of its own, so D-018's testbed page and load
scenario don't apply.

## Build order

1. **P3:** the first version (this document).
2. **Every step:** a new `CHANGELOG.md` entry, re-imported posts, statuses moved in `src/lib/data`.
3. **Before 3.8:** a host and an origin (O-012), and the docs at `/docs/`.

## Open decisions

- Hosting and the public origin (with O-012).
- Whether `src/lib/data` should be generated from `docs/COMPARISON.md` and `docs/ROADMAP.md` instead of transcribed,
  once their tables are stable.
