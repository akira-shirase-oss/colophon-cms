# How developer platforms keep users informed as the product changes, and what Colophon should copy

**Research date:** 2026-10-27. **For:** the Colophon maintainer. **No repository was modified.**

## How to read this report

- **Observed** means I saw it on the company's own page (docs, blog, changelog, GitHub) on 2026-10-27. Source tags
  like `[S1]` point to the numbered list at the end.
- **Search-level** means I saw it only in a search engine's summary of the company's own page, without opening the
  page (usually because the fetch failed or timed out). Treat these as likely but unconfirmed.
- **Not verified** means I looked and couldn't confirm it.
- Pages were read through a fetch tool that summarises them. Quotations are as the tool returned them; where exact
  wording matters, re-read the source.
- Part 2 is observed facts about Colophon's repository. Part 3 is observed facts about the companies. Part 4 is my
  inference from Part 3. **Part 5 is recommendation, not fact.**

Companies studied: the five required (Supabase, Appwrite, Convex, Strapi, Sanity) plus **Payload** and **Directus**,
chosen because they are self-hosted, open-source CMSs with the same upgrade problem Colophon will have and visible
breaking-change practices, and **PostHog**, chosen because its public handbook documents *how* its changelog, roadmap
and feature previews are produced, which is rare. Linear was checked only as a reference for changelog layout.

---

## 1. Summary

### Key patterns per company (detail and sources in Part 3)

| Company | What they do well that is relevant to Colophon |
|---|---|
| **Supabase** | Per-feature changelog fed from a GitHub Discussions category with an Atom feed; every entry typed (*Breaking Change*, *Deprecation*, *Policy* …); breaking changes carry an **exact effective date** (29 to 57 days of notice in the examples checked); feature maturity labels (private alpha → public alpha → beta → GA) with stated guarantees; monthly "Developer Update" roundups; self-hosted `update.sh` with version pinning; five-day Launch Weeks with hackathon and meetups. |
| **Appwrite** | Changelog filterable by product; self-hosted upgrade = one `upgrade` command + `migrate`, with explicit **version-stepping rules** and a per-release "breaking changes to check" list; `X-Appwrite-Response-Format` header so old SDKs keep working; deprecated SDK methods keep working with IDE warnings; a written release policy (whose self-hosted row is visibly out of date); "Init" launch week with daily livestreams. |
| **Convex** | Changelog **merged with a public request board** ("#411 … shipped", with backer counts); a **short video walkthrough per release**; a plain stability promise ("Code developed on Convex 1.0 or later will continue to operate as-is"); self-hosted upgrade documented as two paths (in place, or export/import). |
| **Strapi** | Weekly GitHub releases with fixed emoji sections including "⚠️ Changes to be aware of"; periodic human "release roundup" posts; `npx @strapi/upgrade` with **codemods** and a dry run; one docs page per breaking change, each answering "handled by a codemod?"; a dated support table (v4 EOL 2026-04-30); Canny board with votes; an RFC repository. |
| **Sanity** | **Dated API versions** (`/v2026-06-24/...`) with `X-Sanity-Deprecated` / `X-Sanity-Warning` headers and 410 after removal; Studio release channels (latest ≈ weekly, next, stable) and auto-updates for minor/patch only; deprecated releases get a "This version is deprecated" callout pointing to the fix; every launch item labelled Beta / Closed beta / Coming soon / Experimental. |
| **Payload** | GitHub releases with a "⚠️ BREAKING CHANGES" section that explains each break; a versioning page that is honest about exceptions; a major-version migration guide that starts with an **idempotent codemod** and then gives one before/after section per break; roadmap and RFCs in GitHub Discussions with votes. |
| **Directus** | Says openly it **does not follow semver** ("any release may include breaking changes"); compensates with one cumulative breaking-changes page per major, each item saying who is affected and what to do *before* upgrading; monthly releases, a monthly video show "The Changelog"; moved feature requests from GitHub Discussions to a voting roadmap. |
| **PostHog** | Per-feature changelog with RSS, every entry linked to docs; a documented pipeline (merged PRs → weekly digest → engineer review → publish); a public roadmap where visitors subscribe to items; in-app **feature previews** (concept → alpha → beta → GA) with automatic feedback emails; a monthly product email with 3–4 highlights. |

### Top 10 recommendations for Colophon (Part 5 has the detail and the templates)

1. **Merge the changelog that already exists** (`CHANGELOG.md` on the `site/website` branch, rendered by `apps/site`)
   and keep writing one entry per step in the same branch as the work. Give the `/changelog` page an RSS/Atom feed
   (the blog has one; the changelog doesn't yet). At v0.1, switch to version sections with Keep a Changelog types.
2. **Publish a one-page "Versioning and stability" policy now:** nothing is stable before v0.1; from v0.1,
   `/content/v1`, the rich-text v1 format, the CLI's `--json` v1 results and webhook payloads are stable
   (additive-only), while `/api/v1` is labelled *Preview* until 1.0. Label every capability *Stable / Preview /
   Experimental / Planned*.
3. **Make silent breaking changes impossible:** land the OpenAPI export that D-027 planned and the roadmap carried
   forward (it doesn't exist yet), commit it, and run `oasdiff breaking` against the last release's copy in CI.
   Golden files do the same for CLI JSON and webhooks; the rich-text fixtures already do it for the format.
4. **Keep path-major versioning** (`/api/v1`, `/content/v1`). Don't adopt Sanity-style dated versions or
   Appwrite-style response-format headers; both exist to serve many client versions from one hosted service. A `v2`
   happens only when it can't be avoided, runs beside `v1`, and `v1` then sends `Deprecation` (RFC 9745) and `Sunset`
   (RFC 8594) headers before it returns 410.
5. **Ship every release from one template** (GitHub Release, mirrored on the site): an upgrade-at-a-glance table,
   breaking changes written as *who is affected / what changed / what to do first*, a deprecations table, known
   issues, **the evidence that ran for this release**, verification commands, and a generated compatibility table.
6. **One upgrade guide per minor version,** with a "check whether you're affected" step, a verification step that
   uses Colophon's own receipts, and an explicit rollback rule. Until 1.0, support upgrades **from the previous minor
   only** (Appwrite's stepping rule), and test that path in CI.
7. **A deprecation policy sized for one person:** before 1.0, any minor may break, but every break is labelled and
   where it's cheap the old behaviour keeps working for one more minor with a warning; from 1.0, at least one minor
   and 90 days, removal only in a major. Mark yanked or dangerous releases the way Sanity and Directus do.
8. **Feedback through GitHub only:** Discussions with *Announcements* (maintainer-only, has a feed), *Ideas* (votes),
   *Q&A* and *Show and tell*; `ROADMAP.md` stays the roadmap and `DECISIONS.md` the record of decided RFCs. No Canny,
   no Discord, no community calls yet.
9. **In-product signals that cost little:** the CLI checks for a new release at most once a day and says so on
   stderr (never in `--json` mode), with an opt-out variable; the server never phones home; deprecation headers
   become stderr warnings in the CLI and log lines for the operator; the Studio shows its version and links to the
   changelog.
10. **Launch small and honest:** the first public demo is one launch *day* (video, build-log write-up, pinned
    Discussions announcement, X), not a launch week, and not Show HN until someone can install Colophon from the
    README (HN's own rule). Use it to recruit the independent installer and the five editors that v0.1's definition
    of done needs. Save a five-day "Proof Week" for v0.1, built from the four write-ups v0.1 already requires plus
    the receipts write-up.

---

## 2. Where Colophon starts (observed in the repository, 2026-10-27)

Read from the `colophon-3.5` worktree (the working directory), the `site/website` branch (`colophon-site` worktree)
and the `p3/3.6-modelling` branch (`colophon-3.6` worktree). Nothing was changed.

- **Version:** `Cargo.toml` has `[workspace.package] version = "0.0.0"`. Binaries already report
  `env!("CARGO_PKG_VERSION")` (CLI `--version`, the server's startup log, the worker's `User-Agent`).
- **Changelog:** there is **no `CHANGELOG.md` on `main`** (checked with `git show main:CHANGELOG.md`). The `site/website` branch has a 322-line `CHANGELOG.md`
  that says "Colophon is pre-release: v0.1 is not out, there are no version numbers yet". Its shape is `## <milestone>`
  then `### <step> · <title>`, with metadata lines (`Status`, `Branch`, `Merged`, `Decisions`, `Build log`) and plain
  bullets, including load numbers and failed runs that were kept. `apps/site` parses it at build time
  (`src/lib/server/changelog.ts`) and renders `/changelog`; the file "is the only place an entry is written". The site
  also has `/roadmap` (from a hand-maintained `#lib/data/roadmap.ts`, a second copy of `ROADMAP.md`), `/blog` with
  `rss.xml`, `/compare` and `/open-source`. I found no feed for `/changelog`.
- **Build log:** <https://404human.dev/colophon/> (source `minimal-portfolio/content/colophon/`), one post per step,
  front matter `kind = experiment | failure | release`, `milestone`, `step`, and a summary with `goal`, `built` and
  `proof`.
- **Roadmap and decisions:** `docs/ROADMAP.md` (milestones with proof gates) and `docs/DECISIONS.md` (D-001 …). On the
  3.6 branch, **D-057** (2026-10-27) widens scope to "every feature Strapi and Sanity offer, at Colophon's bar of
  proof" with new milestones R, M, E, D, and moves GraphQL into milestone D; **D-059** requires plain words for
  non-technical editors from one glossary (a changeset is a *publish request*, a revision a *version*, a slug a *URL
  name*, destinations are *website connections*); `docs/COMPARISON.md` holds the gap list against Strapi and Sanity.
- **Commitments already written down:**
  - Module 19: semantic versioning; every release ships "a changelog and a compatibility table", an SBOM, checksums,
    signed provenance and "an upgrade and rollback guide"; support for "the current stable minor and the previous
    minor … with a proposed 90-day transition"; "Breaking changes are announced explicitly. Destructive upgrades never
    run automatically"; OSS2 "No public release before the licence is confirmed" (O-014 still open).
  - Module 04: "Contract changes need compatibility tests and a documented deprecation window"; the error envelope
    has a stable `code`.
  - Module 08: CLI invariant C1, "`--json` prints exactly one versioned result on stdout; nothing else goes to stdout".
  - Module 16: product docs are versioned; DS1 "No page describes a planned feature as available".
  - Module 18: upgrade order "read the release notes → verify the backup → rehearse on staging → apply additive schema
    → deploy compatible binaries → smoke checks → backfill → remove old fields only after the support window".
- **Contracts that will need versioning:** `/api/v1` (management) and `/content/v1` (public delivery, read by
  deployed sites and caches); the `colophon/rich-text` v1 format (with fixtures that must stay valid); CLI `--json`
  results; webhook payloads from the worker; the SDK generated from OpenAPI. **The OpenAPI document (D-027) has not
  been built yet:** the roadmap lists it as carried forward from P1, and there is no `openapi*.json` in the
  checkouts.
- **Repository hygiene:** `git show main:<file>` finds no `CHANGELOG.md`, `LICENSE` or `SECURITY.md` on `main`
  (checked at `d1c9bd7`, "Merge P3 step 3.5"); none of the checkouts has a `CONTRIBUTING.md`.
- **First public demo:** step 3.7, "recorded crash loop plus a live site with receipts". Independent install from
  the README is a v0.1 gate, not a P3 one.

---

## 3. What each company does (observed facts)

Numbered 1 to 7 as in the brief: (1) changelog and release notes, (2) launch moments, (3) docs, versioning and
deprecation, (4) roadmap and feedback, (5) release engineering visible to users, (6) community and channels,
(7) in-product communication.

### 3.1 Supabase

1. **Changelog.** `supabase.com/changelog` lists per-feature entries, newest first, each with a type label (*New
   Feature, Improvement, Breaking Change, Bug Fix, Deprecation, Policy*) and product tags (Auth, Studio, Database, SDK
   names) [S1]. Cadence is several entries a week: eight between 2026-10-05 and 2026-10-23, for example "Scoped
   personal access tokens are now generally available" (10-06) and "[Public Beta] Supabase Knowledge Base" (10-23)
   [S1]. The entries are GitHub Discussions in the org's "🚢 Changelog" category, whose description reads "Platform
   updates. RSS: https://github.com/supabase/supabase/discussions/categories/changelog.atom"; posts carry labels such
   as `breaking-change`, and titles carry stage prefixes like "[Public Alpha]" and "[Private Alpha]" [S3]. (The
   changelog URL `supabase.com/changelog/47796-developer-update-july-2026` and Discussion #47796 are the same post
   [S10].) A monthly **"Developer Update"** roundup was published for January to July 2026 (the latest I found was
   July) [S10, search-level for the month list]. No screenshots or video in the listing [S1].
   **Breaking changes** are labelled and dated. Examples [S2]:
   - TLS 1.2 cipher subset: posted 2026-10-12, effective "November 10, 2026" (29 days).
   - Framework adapters in `@supabase/server`: deprecated 2026-10-05, "will be removed on December 1, 2026" (57 days),
     with the replacement named ("Move to the framework bridges in the frameworks guide").
   - Postgres 14: posted 2026-05-12, support ends 2026-07-01 (50 days); projects are upgraded automatically and
     projects using removed extensions "will be paused".
2. **Launch moments.** `supabase.com/launch-week` currently shows **Launch Week 15**, 14–18 July (no year on the
   page): "Five days of launches", one "Introducing …" headline per weekday on a main stage (JWT signing keys,
   analytics buckets with Iceberg, Branching 2.0, observability, persistent storage for Edge Functions), a "Build
   Stage" of smaller releases already live, a hackathon of 10 days starting Friday, and community meetups in many
   cities via Luma [S4]. I found no Launch Week 16 or later (searched).
3. **Docs and maturity.** The features page labels each feature *GA, Beta, Public Beta* or *Public Alpha* [S7]. The
   stage definitions (search-level; the page's `.md` version returned 404): private alpha is early access by email;
   public alpha means "the API might change in the future, not that the service isn't stable" and is outside the
   uptime SLA; beta is pen-tested and "The API is guaranteed to be stable and there is a strict communication
   process for breaking changes"; GA adds the SLA [S7]. **Self-hosted:** stable Docker Compose snapshots "approximately
   once a month", tested together; `update.sh` updates in place with a three-way merge of your configuration; the
   installed version is recorded in `.supabase-version`; a self-hosted `docker/CHANGELOG.md` plus the `self-hosted`
   label on Changelog discussions; changing individual image tags is possible but "compatibility is **not
   guaranteed**" [S5]. I found no separate deprecation-policy page.
4. **Roadmap and feedback.** No public roadmap found. GitHub Discussions categories include Feature Requests,
   Questions, Polls, Show and tell, Changelog and Troubleshooting, with upvotes [S8].
5. **Release engineering.** The status page moved to incident.io (changelog, 2026-10-08) with subscriptions by email,
   RSS and Slack [S1]. Postgres minor releases that need reindexing are announced as *Breaking Change* [S2].
6. **Channels.** Blog with categories (product, engineering, developers), RSS at `/rss.xml`, and a footer signup "Get
   product updates and news from Supabase." [S6]. Discord, X and YouTube presence: not verified in this session.
7. **In product.** Dashboard **Feature previews**: opt-in toggles from the user menu, also available on self-hosted
   Studio since 2025 (search-level) [S11]. The CLI prints a new-version notice after commands; v2.115.0 added
   `SUPABASE_NO_UPDATE_NOTIFIER=1` to turn it off (seen in a third-party mirror of the GitHub release notes, not on
   GitHub itself) [S9].

### 3.2 Appwrite

1. **Changelog.** `appwrite.io/changelog`: per-announcement entries with date, title, product tags and summary, some
   with a cover image; filters grouped as Products (Auth, Databases, Storage, Functions, Messaging, Sites, Domains,
   Realtime, Firewall), Tools (Console, MCP, Integrations, CLI, SDKs, API) and More (Performance, Security,
   Infrastructure, Programs) [A1]. Version releases are entries that link to GitHub release notes, e.g. "Announcing
   Appwrite 2.6.2 and 1.9.10 for self-hosted deployments" (2026-10-24), with a separate security entry for the same
   day [A1]. No explicit breaking-change label (warnings are in the text); no RSS or email signup seen [A1].
2. **Launch moments.** **Init** is a five-day launch week. The Init 2026 recap (published 2026-09-15): Day 1 Appwrite
   2.0, a redesigned Console and more; Day 2 native PostgreSQL; Day 3 VectorsDB, DocumentsDB, MySQL; Day 4 an
   S3-compatible Storage API; Day 5 Firewall, OAuth2 server, Domains; a livestream each day, AMAs, Discord events, a
   giveaway, an "Init ticket", and a closing party on Discord Stage [A2]. Not everything reached self-hosting at once:
   the S3 API arrived for self-hosted with Appwrite 2.1 [A2]. Dates 31 August–4 September 2026 (search-level).
3. **Docs, versioning, deprecation.**
   - The REST API is `/v1`. The `X-Appwrite-Response-Format` header is a "Version number used for backward
     compatibility … The response will be formatted to be compatible with the provided version number … This helps
     Appwrite SDKs keep backward compatibility with Appwrite server API version" [A3].
   - Release policy [A4]: semver for SDKs and self-hosted; "a new minor version of the product every quarter";
     "standard maintenance support for the last 3 API versions", then security fixes "for an additional ten years";
     SDKs: 5 latest versions; runtimes: 24 months; "Self-hosted versions of Appwrite will receive continued support for
     the latest major version"; self-hosted releases "typically updated about 2 months after major feature releases to
     Cloud". **The table still lists the self-hosted current version as `1.5.x`** while the changelog shows 2.6.2 and
     1.9.10, so the page is stale [A1, A4].
   - Deprecation example: JavaScript SDKs moved to object arguments in `appwrite@19.0.0`; "Positional arguments still
     work, but they've been marked as deprecated. Your IDE will show warnings, and you can migrate over time." No
     removal date [A7].
   - Self-hosted upgrades [A5]: back up; read release notes for breaking changes *for each version you move through*;
     test on a non-production instance; patch upgrades may skip the migration; **minor upgrades step through each
     minor's latest patch** (1.5.1 → 1.5.11 → 1.6.2 → 1.7.4); one command
     (`docker run … --entrypoint="upgrade" appwrite/appwrite:<version>`, avoid `latest`), then
     `docker compose exec appwrite migrate`; a "breaking changes to check" list per release (2.6.2 refuses an empty
     `_APP_JOBS_SECRET` and the upgrade command generates one).
4. **Roadmap.** `appwrite.io/roadmap` returned 404 on 2026-10-27. The community page lists "submit new ideas" via
   GitHub [A6]. A current public roadmap: not verified.
5. **Release engineering.** Two self-hosted release lines patched in parallel (2.6.2 and 1.9.10) [A1].
6. **Channels.** Discord 23K+, X 127K+, GitHub 57.8K stars, YouTube 13K+, "800+ contributors"; no office hours or
   newsletter listed [A6].
7. **In product.** The response-format header and IDE deprecation warnings above [A3, A7]. A Console "what's new":
   not verified.

### 3.3 Convex

1. **Changelog.** `ship.convex.dev`, "What's new with Convex?", "Every update, improvement, and release we've shipped
   — backed by the community, in the open" [C1]. Two entry types: **Release** (e.g. "convex v1.48.0", 2026-10-23,
   with a **"Watch the release walkthrough" video**) and **Improvement**, tied to a numbered public request (e.g.
   "#411 Increase HTTP response size limit to 100 MB", 2026-10-19) with backer avatars. The same site is the request
   board: Requested 93, Planned 11, In progress 4, Completed 275 [C1]. A release entry is an "Overview" bullet list,
   with deprecations inline (1.42.0 marks three types `@deprecated` because the APIs using them were deprecated in
   `convex@1.6.0`) [C2]. No RSS, email signup or breaking label seen [C1]. The npm package's `CHANGELOG.md` has version
   headings, no dates, and bullets starting Added/Fixed/Changed; 1.48.0 says of two deprecated slots "They'll be
   removed in a future version" [C3].
2. **Launch moments.** "Vibe Week", April 7–11: a demo-day livestream, MCP server GA, an Agent component, anonymous
   login (search-level; the page timed out; year not confirmed) [C7].
3. **Stability promise** [C4]: "Code developed on Convex 1.0 or later will continue to operate as-is"; beta features'
   "APIs might change in the future, requiring additional effort when upgrading"; for a breaking change "we will
   contact teams directly to provide substantial advance notice". No timeline.
4. **Roadmap.** The request board on `ship.convex.dev` is the roadmap (Planned, In progress) [C1].
5. **Self-hosted upgrades** [C6]: either in place (run `npx convex export` first, then watch the logs for the line
   each migration names, such as `MigrationComplete(115)`), or export → upgrade the image → import with
   `--replace-all` (downtime; do a test export to estimate it). Support is in Discord's `#self-hosted` channel; the
   open-source repo is synced with internal development "within a handful of days" [C5].
6. **Channels.** `convex.dev/community` redirects straight to a Discord invite [C8].
7. **In product.** Per-release video walkthroughs [C1]. A CLI update notice: not found.

### 3.4 Strapi

1. **Release notes.** GitHub releases **weekly, on Wednesdays**: 5.57.0 (2026-10-07), 5.58.0 (10-14), 5.59.0 (10-21)
   [T1]. Fixed emoji sections: "🚀 New feature", "🔥 Bug fix", "💅 Enhancement", "📚 Documentation Changes", "⚙️ Chore",
   "🚨 Security", "⚠️ Changes to be aware of" (prose notes with "Known issue" labels), "❤️ Thank You", contributors;
   entries link PRs [T1]. A human layer on top: **"release roundup"** blog posts that summarise several minors by
   product area and end with "What's next" (e.g. v5.33–v5.39, published 2026-03-11) [T8], and notices on a
   Canny-hosted changelog (e.g. the v4 end-of-support notice) [T6]. The docs have their own semver'd release notes,
   "generally released weekly, on Wednesdays" [T7].
2. **Launch moments.** A **Strapi 5 Launch Week**, 23–27 September 2024, one theme a day (Draft & Publish and Content
   History; developer experience; Vite and TypeScript; the LaunchPad demo app; multi-environments for Strapi Cloud)
   (search-level) [T9]. StrapiConf was the 2025 announcement moment (search-level, press).
3. **Docs, upgrade tooling, support.**
   - `npx @strapi/upgrade` with `major | minor | patch | latest | to <target> | codemods`; `--dry` runs codemods without
     editing files; **`major` is refused until the app is on the latest minor and patch**; codemods "automatically edit
     the application codebase according to the breaking changes introduced up until the targeted version"; it doesn't
     move files or migrate data (data migration runs on first start of the new version) [T2, T3].
   - The v4 → v5 guide has a step-by-step guide, the upgrade tool, and **one page per breaking change**. Each page uses
     the same template: "Breaking change description", "Migration", "Notes", "Migration steps", "Is this breaking
     change affecting plugins?", "Is this breaking change automatically handled by a codemod?" [T3, T4].
   - v4 timeline: active support ended at the end of October 2025, then "a 6-month security only maintenance period",
     EOL at the end of April 2026; the FAQ says "Strapi v4 reached end of life on April 30, 2026" and 4.26.2 is the final
     v4 release [T3, T6]. The `SECURITY.md` table gives each major's support start, support end, security-until date
     and status [T5].
4. **Roadmap and feedback.** `feedback.strapi.io` (Canny): boards with counts (Content Editing XP 329, Developer
   Experience 379, Plugin Requests 308, …), a roadmap of Planned / In Progress / Complete with vote counts (e.g.
   "Responsive Administration Panel" 695), and a changelog tab [T6]. An RFC repository, `strapi/rfcs`: copy the
   template, open a PR, the core team "will attempt to review some portion of the open RFC pull requests" each week;
   an accepted RFC is agreed "in principle", not scheduled [T11].
5. **Security.** Reports only through GitHub Security Advisories; detailed disclosure delayed "from 2 to 8 weeks
   depending on the severity"; paid customers get pre-disclosure notices, free users learn at public disclosure via the
   advisory, CVE, release notes and blog [T5].
6. **Channels.** Discord, the plugin marketplace, YouTube, Twitch, events, and the feedback site; no forum, community
   calls or newsletter listed on the community page [T10].
7. **In product.** "Update notification logging" can be switched off with `logger.updates.enabled` in the server
   config, which replaced `STRAPI_DISABLE_UPDATE_NOTIFICATION` in v5 [T4]. An admin-panel "new version" banner: not
   verified.

### 3.5 Sanity

1. **Changelog.** `sanity.io/changelog`: one entry per product release, titled "Product version: summary": Sanity
   Studio v6.20.0 (2026-10-19), MCP server v2.42.2 (2026-10-26), and "Manage v2026-10-19", a product versioned by date
   [N1]. Per-product RSS and JSON feeds ("RSS (Latest)", "JSON Feed (Full)" …) (search-level) [N8]. No breaking label in
   the index; deprecations sometimes appear in headlines [N1]. Entries carry status callouts: the v6.10.1 entry is
   marked **"This version is deprecated"** because of a data-loss bug, and points to 6.14.1 and `npm install
   sanity@latest` [N5]. Docs pages list "Related changelog entries" [N3].
2. **Launch moments.** The "Everything *[NYC]" 2026 event recap lists seven announcements, **each with a status**
   (Beta, Closed beta, Coming soon, Experimental) and either a docs link or an early-access form [N7]. Majors get
   their own post: "Sanity Studio v6: A focused upgrade" (2026-06-09) gave the pre-release on `next`, the date `latest`
   would move (Thursday, June 11), every breaking change with its opt-out, and an "Are We V6 Ready" tracker for
   plugins; no codemods [N4].
3. **API versioning and deprecation** [N2]:
   - "Every Sanity API request is pinned to a version, written as a date" in the path (`/v2026-06-24/data/query/…`);
     any date resolves to the latest version released on or before it; "Write the date as a literal string", because
     computing it means "your API version changes every day".
   - Omitting the version gives a deprecation warning and defaults to `v1`; `vX` is experimental ("may change at any
     time in any way") and answers with an `X-Sanity-Warning` header.
   - A list of changes Sanity treats as backward-compatible (new response attributes, new `_`-prefixed metadata, new
     optional parameters, new headers, new endpoints, new GROQ functions).
   - Deprecated versions are announced on the website and by email and return `X-Sanity-Warning` and
     `X-Sanity-Deprecated: true`; after removal "all calls to that version will return errors with code `410`". No
     fixed notice period.
   - Studio v2's standard support ended on 2023-12-07 (search-level) [N10].
4. **Roadmap and feedback.** No public roadmap found. Product discussion happens in Discord product channels, where
   "you'll often meet our product managers and engineers, who will both ask for feedback" [N6].
5. **Release channels** [N3]: *latest* ("usually released once a week"), *next* (as merged), *stable* ("when a version
   has been in the latest channel for about a week without any reported issues"), or a pinned version. Auto-updating
   Studios get minor and patch releases automatically; majors with runtime effects don't roll out automatically [N3,
   N4]. Breaking changes "will be announced clearly and in a timely fashion" [N3].
6. **Channels.** The 35,000-member Slack was deleted for Discord on 2025-08-08: `#help`, `#getting-started`,
   `#deep-dives`, `#ask-ai`, product channels, `#announcements`, and "a full time community support engineer" [N6].
7. **In product.** Auto-updating Studios show a "Reload to update to vX" action (evidenced by the bug-fix entry about
   it) [N5]; `sanity dev` "may occasionally prompt you to keep your Sanity Studio dependencies up to date", and builds
   warn when the local version is out of sync with the deployed one [N3]; the API deprecation headers above [N2].

### 3.6 Payload

1. **Release notes.** GitHub releases: weekly 3.x minors (3.91.0 on 2026-10-15, 3.92.0 on 2026-10-22) and 4.0 betas
   every few days [P1]. Emoji sections ("🚀 Features", "🐛 Bug Fixes", "⚡ Performance", "🛠 Refactors", "📚
   Documentation", "🧪 Tests", "⚙️ CI", "🏡 Chores") and **"⚠️ BREAKING CHANGES"**, where each break repeats the bullet
   and adds a paragraph (e.g. "Raises the minimum supported Node.js version to `24.22.1`"), sometimes with migration
   steps; bold scope prefixes (`**ui:**`, `**drizzle:**`); backports marked "(3.x)"; contributor list [P1]. Majors
   and betas get blog posts (Payload 4.0 beta, search-level) [P7].
2. **Launch moments.** The first Launch Week, November 2022. Day 2 combined a $4.7M seed round, six small features and
   "Over 40 fixes and enhancements" in 1.1.25, said "Payload will always be free and open-source", and pointed to
   roadmap items labelled `planned` in GitHub Discussions [P6].
3. **Versioning and migration** [P2, P3]:
   - Major = "Breaking changes" (follow the migration guide and run the codemod); minor = "New features,
     deprecations, bug fixes", usually safe, but "Minor releases can still change anything listed under Exceptions";
     patch = "Guaranteed safe to upgrade without code changes"; "Canary and beta builds make no promises to each
     other"; "Migration guides only cover changes between stable versions".
   - The 3.0 → 4.0 guide opens with the codemod, `npx @payloadcms/codemod`, "idempotent and safe to run on a
     partially-migrated project", with per-transform flags; then pre-upgrade steps (Node, Next.js and TypeScript
     minimums; finish the Slate → Lexical migration on 3.x first); then one section per breaking change with a
     before/after example and "Cases the codemod leaves behind"; removed APIs are described as deprecated in 3.x.
4. **Roadmap and feedback.** GitHub Discussions with Announcement, "Feature Requests & Ideas" (votes), Roadmap, Polls,
   Q&A and Show and tell; a pinned "Payload Product Roadmap" and a pinned "Payload is joining Figma!"; RFCs posted as
   discussions ("RFC: Teach Payload which URL a document lives at") [P5].
5. **Security.** On 2026-09-18 Payload patched both lines at once (3.90.0 and 4.0.0-canary.34), linked the GitHub
   advisories ("affected versions, severity, impact, and upgrade guidance"), and said "We recommend updating as soon as
   possible"; the 3.90.0 minor included "several necessary breaking changes" with migration steps [P4].
6. **Channels.** Discord is named for questions in the security post [P4]; `payloadcms.com/community` returned 404.
7. **In product.** Not verified.

### 3.7 Directus

1. **Release notes.** GitHub releases with "⚠️ Potential Breaking Changes", "✨ New Features & Improvements", "🐛 Bug
   Fixes & Optimizations" and "📦 Published Versions", grouped by package (`@directus/app`, `@directus/api`,
   `@directus/sdk`), each item with its PR and contributor [D3]. Recent: 12.4.1 (Sep 23), 12.5.0 (Oct 7), 12.5.1
   (Oct 23) [D3]. The releases Atom feed exists (`github.com/directus/directus/releases.atom`, fetched) [D3].
2. **Versioning, stated plainly** [D1]: "we use a three number versioning scheme", "we don't adhere to semantic
   versioning stadards [sic]", "any release may include breaking changes", "we use the importance or impact of changes
   to determine the version number"; releases "approximately once a month".
3. **Breaking-change docs** [D2]: one page per major (Version 10, 11, 12), listing every minor's breaking changes;
   each item says what changed, who is affected and what to do before upgrading. Examples: for 12.5.0, "Before
   upgrading, review the `ip_access` column of `directus_policies` and correct any invalid entries" (otherwise every
   request from an affected user fails with 500, logins included); for 12.4.0, upgrade to 12.4.1 instead because of a
   regression. Self-hosted: change the Docker image tag, migrations run automatically, pin a specific version in
   production; Cloud projects update in rolling releases with advance notice of significant breaking changes
   (search-level) [D7].
4. **Launch moments.** "Leap Week": 1 (23–27 October 2023), 2 (March 2024), 3 (June 2024, including the Directus 11
   release candidate and extended security updates for 10.12 to the end of 2024) (search-level) [D6].
5. **Roadmap and feedback.** Feature requests moved from GitHub Discussions to a voting roadmap on Productlane
   (`roadmap.directus.com`), which "shows what we're considering, not the order we'll build them"; "We prioritize
   heavily based on community feedback" [D5]. A Directus video explains the move: the GitHub process "wasn't so
   integrated with their development workflows" (search-level) [D5].
6. **Channels.** **"The Changelog", a monthly Directus TV show** (25–45 minutes) where the team walks through core
   releases, new content and community work; the listing has a gap between September 2025 and February 2026 [D4].
7. **In product.** Not verified.

### 3.8 PostHog

1. **Changelog.** `posthog.com/changelog`: per-feature entries with a team, a category, a short description and usually
   a "Docs" link; the page covers 12 months with yearly archives, is "Regenerated on every deploy", and has RSS at
   `/changelog.rss` [H1]. **The process** [H2]: a Cloudflare Worker records merged PRs across 26 repositories daily;
   an AI agent ranks each team's week and "surfaces at most 10 items per team"; every Thursday a Slack digest per team;
   engineers review their cards ("you're the one who knows whether it's right"); "Publish" opens an issue in
   `PostHog/changelog-drafts`, whose GitHub Action posts it; titles "short and sweet", screenshots or GIFs encouraged;
   "if you'd want a user to know about it, it belongs in the changelog".
2. **Launch moments.** GA launches are led by marketing as product announcements [H5]. (Launch weeks not researched.)
3. **Maturity stages** [H5]: concept (3–6 months out; waitlist), alpha (invited users), beta (open opt-in; needs a
   title and description, a "Give feedback" button, beta-marked docs and an owner; "clearly marked as such in the
   UI"), GA (pricing, quality bar, rollout plan).
4. **Roadmap** [H4]: items without a date are "Under consideration", with a projected date "In progress", with a
   completed date "Recently shipped"; GitHub issues can be attached and drive progress; visitors subscribe to
   in-progress items, and "Notify subscribers" drafts an email that someone sends by hand.
5. **Release engineering.** Not researched.
6. **Channels** [H3]: a product email (the page says both monthly and bi-weekly) with "three or four of the most
   impactful changes", sent to the "Recently Engaged (4 months)" segment and tagged "Product updates" so people can
   unsubscribe from that category alone; a separate Substack newsletter.
7. **In product** [H5, H6]: **feature previews** in settings — a "Coming soon" tab to register interest and a
   "Previews" tab to toggle alpha and beta features, per user; automatic feedback emails (alpha immediately, beta
   after 5 days); opting in or out overrides the flag's other release rules (search-level for the last point).

### 3.9 Linear (format reference only)

An entry every 1–2 weeks (2026-08-20 … 2026-10-22), each a hero feature with a screenshot or video, optional further
feature blocks, then "Improvements" and "Fixes" grouped by product area, with docs links [L1]. No feed was visible in
the fetched content.

### 3.10 Side by side

| | Granular log | Human layer | Breaking changes live in | Versioning | Deprecation signal to software | Feedback | Launch moment |
|---|---|---|---|---|---|---|---|
| Supabase | Changelog = GitHub Discussions (Atom) | Monthly Developer Update | Typed entries with effective dates | Feature stages; self-hosted snapshots | CLI notice (opt-out) | Discussions + votes | Launch Week (5 days + hackathon) |
| Appwrite | GitHub releases | Changelog site with filters | Per-release upgrade notes | `/v1` + response-format header; quarterly minors | IDE `@deprecated` | GitHub | Init (5 days, livestreams) |
| Convex | npm `CHANGELOG.md` | ship.convex.dev + videos | Inline; teams contacted directly | 1.0 promise | — | Own request board | Vibe Week (search-level) |
| Strapi | Weekly GitHub releases | Release roundups, Canny notices | One docs page per break + codemods | Semver, LTS table | Update-notice log config | Canny + RFC repo | Launch Week with v5 |
| Sanity | Per-product changelog (feeds) | Blog per major, event recap | Release blog + API rules | Dated API versions; release channels | `X-Sanity-Deprecated`, 410 | Discord | Annual event |
| Payload | Weekly GitHub releases | Blog for majors | "⚠️ BREAKING CHANGES" + migration guide + codemod | Semver with exceptions | — | Discussions + votes | Launch Week (2022) |
| Directus | Monthly GitHub releases | Monthly video show, monthly summary | Cumulative page per major | Not semver (by design) | — | Productlane votes | Leap Week |
| PostHog | Merged-PR pipeline | Per-feature changelog + monthly email | (not researched) | Feature stages | Feature previews | Roadmap subscriptions, GitHub | Product announcements |

---

## 4. Patterns across the eight (inference)

1. **Everyone runs two layers.** A granular, complete log for people upgrading (GitHub releases, `CHANGELOG.md`) and a
   short, curated layer for people deciding whether to care (Supabase's monthly update, Strapi's roundups, Directus's
   monthly show, PostHog's email, Convex's videos, Sanity's major-version posts). Neither layer replaces the other.
2. **Breaking changes get a cumulative home outside the release stream.** Directus's per-major page, Strapi's page per
   change, Payload's migration guide, Supabase's typed and dated entries. People skip versions; release notes read one
   at a time don't tell them what they skipped.
3. **Dates beat adjectives.** Supabase gives the exact day (29–57 days ahead in the examples); Strapi publishes a dated
   support table. Sanity ("appropriate notice"), Convex ("substantial advance notice") and Appwrite ("migrate over
   time") promise without numbers. A number you can keep is more useful than a generous number you can't.
4. **Semver is aspirational; the label on each change is what matters.** Directus abandons semver openly; Payload
   ("Minor releases can still change anything listed under Exceptions") and Strapi ("Changes to be aware of") ship
   behaviour changes in minors; Payload's security minor 3.90.0 had breaking changes. Users forgive a break that was
   labelled; they don't forgive one that wasn't.
5. **Maturity labels are the cheapest trust tool.** Supabase, PostHog, Sanity and Convex all label features (alpha,
   beta, GA, experimental) and attach a promise to each label.
6. **Self-hosted upgrades converge on one shape:** pin a version; back up; read the notes for every version you pass
   through; one upgrade command; migrations automatic or one command; a rule about which versions you may skip
   (Appwrite steps through minors; Strapi refuses a major until you're on the latest minor; Convex offers
   export/import when in-place fails).
7. **Software tells users at the point of use.** Sanity's deprecation headers, Appwrite's IDE warnings and response
   format header, Supabase's CLI notice with an opt-out, Sanity's Studio reload prompt, Strapi's update-notice switch.
   These reach people who never read changelogs.
8. **Launch weeks need a team.** Daily livestreams, hackathons, meetups, giveaways, tickets and Discord stages
   (Supabase, Appwrite) take many people. The small-scale versions are a short video per release (Convex), a monthly
   show (Directus), or a launch week that is mostly fixes plus one big announcement (Payload 2022).
9. **The best feedback loops close visibly.** Convex shows the request number and its backers on the changelog entry
   that ships it; PostHog lets people subscribe to a roadmap item and emails them. Boards without that loop are just
   inboxes.
10. **Policy pages rot unless generated or reviewed on a schedule.** Appwrite's release policy still says self-hosted
    is on `1.5.x`.
11. **At scale the changelog is automated, but the principle is small:** the person who made the change drafts the
    entry when it merges, and someone edits on a schedule (PostHog).

---

## 5. Playbook for Colophon (recommendations)

### 5.1 Principles

- **Same rules as the code: proof, not adjectives.** Colophon's identity is evidence. A changelog entry that makes a
  guarantee names the test or load run behind it; the existing changelog already does this in prose.
- **Plain words first (D-059).** Write the first line of every entry for an editor, using the glossary: *publish
  request*, *version*, *URL name*, *website connections*. Put API names, routes and decision numbers on the next line.
- **Promise only what one person can keep (OSS4).** Fewer, dated promises.
- **One source per fact.** The changelog lives in `CHANGELOG.md`, the roadmap in `ROADMAP.md`, decisions in
  `DECISIONS.md`, the compatibility table in a generated file. Pages render them; nothing is typed twice.
- **The AI pair drafts, the maintainer approves.** PostHog's pipeline scaled down to one repository: the agent that
  writes the code writes the changelog line in the same branch, as it already does for module docs.

### 5.2 What to do when

| When | Do | Cost |
|---|---|---|
| **Now** (pre-release) | Merge the existing `CHANGELOG.md` and keep one entry per step; add a feed to `/changelog` | ~1 h |
| Now | Write editor-facing lines in D-059's plain words; technical detail second | habit |
| Now | Publish a "Versioning and stability" page (5.6) and label capabilities *Stable / Preview / Experimental / Planned* on the site and in docs (extends DS1) | ~2 h |
| Now | Enable GitHub Discussions: *Announcements* (only maintainers can start one [G6]; the category has an Atom feed, as Supabase's does [S3]), *Ideas* (votes), *Q&A*, *Show and tell*; issue templates send ideas there | ~1 h |
| Now | Make the site's `/roadmap` read one source instead of the hand-kept `#lib/data/roadmap.ts` copy of `ROADMAP.md` | ~2 h |
| Now | Build the D-027 OpenAPI export it was planned in P1; commit it; run `oasdiff` in CI as a warning | part of planned work |
| Now | Settle the licence (O-014) before inviting anyone to try Colophon; until then the README says so | decision |
| Now | Write the release-notes and upgrade-guide templates (5.4, 5.7) into the repo so v0.1 isn't the first use | ~1 h |
| **v0.1** | Switch `CHANGELOG.md` to version sections with Keep a Changelog types (5.3); keep the step history below | ~1 h |
| v0.1 | GitHub Release per version from the template, mirrored on the site; the releases Atom feed comes free | per release |
| v0.1 | Generated compatibility table attached to every release (module 19 already requires one) | ~3 h once |
| v0.1 | Upgrade guide per minor; CI rehearses the upgrade from the previous minor's tag (module 18's rehearsal) | per release |
| v0.1 | `/content/v1` becomes additive-only: `oasdiff breaking` fails CI for it; `/api/v1` stays *Preview* | config |
| v0.1 | Deprecation machinery: `Deprecation` + `Sunset` + `Link rel="deprecation"` headers from one table, a rate-limited server warning, CLI warnings on stderr, 410 after removal | ~1 day |
| v0.1 | CLI update check (5.9) and the server's no-phone-home promise written down | ~half a day |
| v0.1 | `SECURITY.md` with GitHub private advisories (module 19 already plans it) and a supported-versions table | ~1 h |
| v0.1 | "Proof Week" launch (5.8) | one week |
| **Later**, when people depend on releases | Security fixes for the previous minor for 90 days (the spec's window) | ongoing |
| Later | Studio "What's new": the bundled changelog shown once per version per person, no network | ~1 day |
| Later | Studio feature previews: experimental features behind a per-project switch, labelled | when needed |
| Later | `colophon upgrade check`: finds data and settings an upgrade would affect (Directus's "review this column", done by a program) and refuses unsupported version jumps | when needed |
| Later | A monthly roundup post (build log) and an email from its feed, once there are readers who aren't following daily | monthly |
| Later | GraphQL (milestone D, D-057): the spec's `@deprecated(reason:)` on fields and the same additive-only rule | with GraphQL |
| Only if forced | Codemods, Discord, community calls, dated API versions, a `v2` | — |

### 5.3 Changelog entry format

**Now:** keep the existing per-step shape on the `site/website` branch (it already matches what the parser in
`apps/site` reads). Two small additions:

- a `- **Proof:**` metadata line when the entry makes a guarantee (fault case IDs, a load-result file);
- an `- **Affects:**` line once someone other than the maintainer runs Colophon (`editors`, `developers`,
  `operators`).

**From v0.1:** version sections, Keep a Changelog 1.1.0 types [G1], with Colophon's rules:

```markdown
# Changelog

All notable changes to Colophon are recorded here, newest first. The format follows
[Keep a Changelog 1.1.0](https://keepachangelog.com/en/1.1.0/) and Colophon uses
[Semantic Versioning](https://semver.org/). While the version is 0.y.z, any minor release may
contain breaking changes; every one is marked **Breaking**. See docs/versioning.md.

## [Unreleased]

### Added
- **Studio:** You can take an article off the website. The removal goes through review like any
  other publish request, and the article stops being served at every address, old ones included.
  Technical: tombstone release; `/content/v1` refuses the entry at any `?generation=`. (#41, D-049;
  proof: F07)

### Changed
- **Breaking · API:** Publishing a publish request now needs the release number you last saw.
  Technical: `POST {P}/changesets/{id}/publish` requires `base_generation`; without it the answer is
  `422 missing_base_generation`. What to do: send the value of the `Colophon-Generation` header you
  read. (#38, D-045)

### Deprecated
- **CLI:** `colophon content list --all` is deprecated; use `--limit`. It still works and prints a
  warning on stderr. Removal: 0.4.0. (#44)

### Removed
### Fixed
### Security

## [0.1.0] - 2027-MM-DD
...

## Pre-release history (steps 0.1 to 3.x)
<the current per-step entries, unchanged>
```

Rules for an entry:

1. One entry per change a user could notice. Internal refactors, CI and test-only work stay in git and the build log.
2. Starts with **Area:** — Studio, Site, Delivery, API, CLI, SDK, Worker, Operations, Docs — and `Breaking ·` when it
   breaks something.
3. First sentence in D-059's plain words: what you can now do, or must now do. Then `Technical:` with routes, fields
   and codes.
4. A breaking entry always says old behaviour → new behaviour → what to do.
5. Links: the PR or issue, the D-number when a decision is behind it, and `proof:` when it states a guarantee.
6. Written in the same branch as the change. A CI check can require that a change under `crates/`, `apps/` or
   `packages/` either touches `CHANGELOG.md` or carries a `no-changelog` label.
7. A bad release stays listed and is marked: `## [0.2.1] - 2027-03-04 [YANKED]`, plus a sentence naming the fix
   release (Keep a Changelog's rule [G1]; Sanity's "This version is deprecated" callout [N5]; Directus's "upgrade to
   12.4.1 instead" [D2]).

### 5.4 Release notes template

Used for the GitHub Release body and the site's release page. Everything below "All changes" is copied or generated,
so writing a release is mostly the first three sections.

```markdown
# Colophon 0.N.0 — <what this release lets you do, in one line>

Released YYYY-MM-DD. <Two to four sentences in plain words. One screenshot or a 30–90 second
screen recording (Convex attaches a walkthrough to every release).>

## Upgrade at a glance

| | |
|---|---|
| Upgrade from | 0.(N-1).x only. On anything older, upgrade to 0.(N-1) first. |
| Database migrations | 00xx to 00yy: additive only / includes a removal step |
| Downtime | none expected / stop the worker before migrating |
| Rolling back | safe to 0.(N-1) binaries / not after migration 00yy: restore your backup |
| Breaking changes | <number>, listed below |
| Guide | docs/upgrading/0.N.md |

## Breaking changes

### <Title in plain words>
- **Who is affected:** <e.g. sites that build from `/content/v1` with cursors older than 0.(N-1)>
- **What changed:** <old → new>
- **What to do before upgrading:** <steps; a query or command that tells you if you're affected>

## Deprecations

| What | Use instead | Warns since | Removed in |
|---|---|---|---|

## Highlights
<Two or three items, each a short paragraph.>

## All changes
<This version's section of CHANGELOG.md, verbatim.>

## Known issues and limits
<What doesn't work yet or misbehaves, honestly (DS1). Strapi calls this "Changes to be aware of".>

## Evidence for this release
- `scripts/check.sh` at <commit>: <passed>
- Fault cases run: F01–F0x; crash loop: <N iterations, result>
- Simulator: <N seeds, 0 failures>; load: <links to perf/results files, machine named>
- Missed or failed runs kept: <links>

## Verify the download
<checksums, signature and SBOM commands (module 19)>

## Compatibility
<generated table: server and CLI version; `/api/v1` OpenAPI file hash and status (Preview);
`/content/v1` (Stable since 0.1); rich text `colophon/rich-text` v1; CLI `--json` v1; webhook
payload v1; SDK version; PostgreSQL and Valkey versions tested; last migration number>

## Thanks
```

### 5.5 Deprecation policy

**Before 1.0 (0.y.z)** — semver's own rule is that "Anything MAY change at any time" in 0.y.z [G2], so the promise is
about labelling, not stability:

1. Any minor release may contain breaking changes. Every one is marked **Breaking** in the changelog and explained in
   the release notes (who is affected, what changed, what to do). No silent breaks: the contract checks in 5.6 fail
   the build if a break has no entry.
2. Patch releases never break anything. If a security fix forces a break (Payload's 3.90.0 [P4]), the release notes
   open with it and say why.
3. Where keeping the old behaviour for one more minor is cheap, keep it and warn (5.6 explains the warnings). Where it
   isn't, break in one step and say so.
4. Data is never changed destructively without the operator acting (module 19). Removing a column or field happens at
   least one minor after the release that stopped using it (module 18's expand, migrate, contract order).
5. Stored formats are permanent: every valid `colophon/rich-text` v1 document stays valid (already a rule). A new
   format is added beside the old one, never in place of it.
6. Support: the latest minor only. Upgrades are supported from the previous minor.

**From 1.0:**

1. A deprecation is announced in a minor release, with its replacement and its removal version and date.
2. The deprecated thing keeps working for **at least one further minor release and at least 90 days**, whichever is
   later (90 days is the spec's transition window), and is removed only in a major release.
3. Signals, all generated from one table in the code: the changelog's *Deprecated* section; a docs page listing every
   deprecation with announced and removal dates; `Deprecation` and `Sunset` headers plus `Link: <guide>;
   rel="deprecation"` on affected routes (RFC 9745 says `Sunset` must not be earlier than `Deprecation` [G3]); a server
   warning, at most once per route per hour, so the operator sees who still calls it; the CLI prints the warning on
   stderr; the SDK marks it `@deprecated`.
4. After removal, the route answers `410 Gone` with the error envelope (`code: "removed"`) and a link to the guide,
   as Sanity does [N2].
5. Support: the latest minor gets fixes; the previous minor gets security fixes for 90 days after the next minor
   ships (module 19's window). If capacity shrinks, say so and narrow it (module 19's own failure mode).

### 5.6 API versioning recommendation

**Keep path-major versions: `/api/v1` and `/content/v1`.**

- **Not dated versions.** Sanity's dated versions [N2] work because Sanity hosts every version for every customer
  and has a team to keep them running. For a self-hosted project with one maintainer, every live version is code to
  keep, test and secure.
- **Not a response-format header.** Appwrite's header [A3] keeps many SDK releases working against one cloud.
  Colophon ships its server, CLI and SDK together; keep them in lockstep (same minor) and the problem doesn't arise.

**Give each contract its own stability level** (the Supabase and PostHog pattern), declared in one place and shown in
the docs:

| Contract | Before v0.1 | From v0.1 | Why |
|---|---|---|---|
| `/content/v1` (public delivery) | Preview | **Stable: additive only** | Deployed sites, CDNs and caches depend on it; it's small (lists, entries, `?generation=`, cursors, ETags); the maintainer's own site is its first user |
| `/api/v1` (management) | Preview | **Preview until 1.0**; breaks allowed with a Breaking entry | Milestones R, M, E and D (D-057) will reshape roles, modelling and editorial routes |
| `colophon/rich-text` v1 | Stable | Stable (permanent) | Stored data |
| CLI `--json` results (C1) | Preview | Stable | Scripts and agents parse them |
| Webhook payloads | Preview | Stable, with a version field in each payload (add one if there isn't one) | Receivers are other people's code |
| GraphQL (milestone D) | — | Preview until it has users | Evolve with `@deprecated(reason:)`, never version the schema |

**What counts as breaking inside a stable `v1`** (adapted from Sanity's list of backward-compatible changes [N2]):

- Breaking: removing or renaming a route, field, parameter or error `code`; changing a type or a meaning; making
  something optional required; **adding a required precondition to an existing route** (`If-Match`,
  `Idempotency-Key`); changing cursor or `?generation=` semantics; narrowing validation; changing what a cache header
  promises.
- Not breaking: new routes; new optional parameters; new response fields; new headers; new error codes on new routes.
  The docs say clients must ignore fields they don't know.

**Machine checks:**

- Land the D-027 OpenAPI export, commit `docs/api/openapi.json`, and run `oasdiff breaking` (Apache-2.0, has a GitHub
  Action [G5]) against the file from the last release tag. For `/content/v1` from v0.1, a finding fails CI; for
  `/api/v1`, a finding requires a **Breaking** changelog entry in the same PR. `oasdiff changelog` can draft the API
  part of the release notes [G5].
- Golden-file tests for CLI `--json` results and webhook payloads, in the style of the rich-text fixtures.
- A stability marker per route in the OpenAPI document (a tag or a vendor extension, whichever `utoipa` supports
  cleanly; not checked).

**Version disclosure:** report the server version on authenticated routes (`/me` or `/api/v1/server`) and in `colophon
doctor`, not in a header on public `/content/v1` answers, where it only helps someone fingerprint unpatched servers.

**If a `v2` ever happens:** serve it beside `v1`, mark `v1` with `Deprecation` and `Sunset` headers [G3], keep it for at
least one major release cycle and 90 days, then answer 410.

### 5.7 Upgrade guide template (`docs/upgrading/0.N.md`)

```markdown
# Upgrading to Colophon 0.N

Applies to 0.(N-1).x → 0.N.x. Time on the reference profile: ~X minutes. Downtime: <none / Y>.

## Before you start
- [ ] Read the 0.N release notes, especially "Breaking changes".
- [ ] Note your versions: `colophon-server --version`, `colophon --version`.
- [ ] On 0.(N-2) or older? Upgrade to 0.(N-1) first. Versions can't be skipped before 1.0.
- [ ] Take a backup and restore it somewhere once. A backup you haven't restored is a hope.
- [ ] If you can, rehearse on a copy first.

## Are you affected?
For each breaking change, a check that answers yes or no, e.g.:
`colophon admin check <thing>` or the SQL query to run. (Directus tells people to "review the
`ip_access` column" by hand [D2]; Colophon can ship the query.)

## What changes for you
- **Editors:** <plain words; screenshots>
- **Developers** (API, SDK, your site's build): <before/after code for each change>
- **Operators** (settings, environment variables, PostgreSQL/Valkey versions): <list>

## Steps
1. Stop the worker (`--role worker`), so no job runs across the upgrade.
2. Apply migrations 00xx to 00yy: <command>. They are <additive / include a removal step>.
3. Replace the binaries; start the server, then the worker.
4. Wait for `/readyz` to report ready.

## Check it worked
- `colophon whoami` answers with your identity.
- Publish a test entry in a scratch project and watch its receipt reach
  *Verified at N locations*.
- Your site still builds from `/content/v1` (run its build once).

## Rolling back
- Before migration 00yy ran: redeploy the 0.(N-1) binaries.
- After it ran: restore the backup (you lose writes made since it), following the operations guide.

## Known issues
```

### 5.8 Launch plan for the first public demo (step 3.7), and for v0.1

**What the first demo is:** a recorded crash loop and a live site with receipts. Strangers can watch it but can't yet
install Colophon from the README (that's a v0.1 gate). So it is a demonstration, not a release: no version number, no
Show HN (Hacker News: "Off topic: blog posts, sign-up pages, newsletters, lists, and other reading material" and "If
it isn't ready, wait and post later" [G7]), and no launch week.

**Prepare (the week before):**

- **Video, 2–3 minutes, unedited terminal where possible:** publish from the Studio; `kill -9` the server and worker
  repeatedly during saves and publishes; the checker's output (no accepted edit lost, no release run twice, no reader
  saw a mixed release); the live site's `/colophon` page and the receipt saying where it was verified. Put the numbers
  on screen.
- **Tag the demo commit** (for example `demo-1`) so anyone can see exactly what the video ran. A tag for
  reproducibility, not a release.
- **Write-up** on the build log: what "published" means in Colophon, the video, the exact commands, and a list of
  what isn't done.
- **Repository front door:** a README status block ("Pre-release. Not for production. What works today / what's
  next" linking `ROADMAP.md` and `/changelog`); the licence decided (O-014) or stated as pending; Discussions enabled;
  a pinned *Announcements* draft ready.
- **One clear question to ask**, for example "Would you trust this receipt? What would you need to try Colophon on a
  side project?" Specific questions get answers; "feedback welcome" doesn't.

**Launch day:**

- Publish the build-log post with the video; publish the pinned *Announcements* discussion linking the post, the video,
  the roadmap and an *Ideas* thread.
- Post on X with the video uploaded natively and the link in a reply, scheduled through PostSyncer at least 12 hours
  ahead so the maintainer can veto (the `x-growth/` routine and guardrails apply).
- Answer every reply and comment for 48 hours (`x-growth/README.md` notes that X drops posts older than 48 hours from
  For You).

**The following days (optional, only with energy to spare):** one short post a day, each explaining one proof
already in the video (the outbox, release generations, generation-keyed cache, receipts). Material that already
exists, not new work.

**One week later:** a short recap in Discussions: what people asked, what changes in the roadmap because of it. Count
what matters: concrete feedback items, people who asked to try it, and anyone willing to be **the independent
installer** or one of **the five editors** for v0.1's usability gate. Recruiting them is the demo's most valuable
outcome.

**v0.1: "Proof Week" (five days, one maintainer):** v0.1 already requires "one technical write-up per hard problem
(outbox, release generations, the simulator, cache invalidation)" and the docs plan a fifth on delivery receipts. Publish
one a day, Monday to Thursday, and on Friday the v0.1 release with notes, upgrade guide, verification instructions and
a Show HN (now that it installs from the README). No livestreams, hackathons, meetups or giveaways: those are the
team-sized parts of Supabase's and Appwrite's weeks [S4, A2].

### 5.9 In-product communication, sized for one person

- **CLI:** check GitHub's latest release at most once a day; cache the answer; print one line on **stderr**
  ("Colophon 0.5.0 is available; you have 0.4.2. Release notes: <url>"). Never in `--json` mode, never when stdout or
  stderr isn't a terminal, never in CI; turn it off with `COLOPHON_NO_UPDATE_CHECK=1` (Supabase added exactly this
  kind of switch [S9]). This keeps C1 intact.
- **Server:** never phones home; say so in the docs. Operators learn about releases from the Atom feeds and the CLI.
- **Deprecations at the point of use:** the headers in 5.5 become a rate-limited server warning for the operator and a
  stderr line in the CLI; the SDK marks deprecated calls so editors show them (Appwrite's approach [A7]).
- **Studio:** the version in the account menu or footer, linked to the changelog. Later, a "What's new" panel built
  from the changelog bundled into the release (no network call), shown once per version per person, in plain words.
- **Maturity labels in the Studio:** anything not *Stable* carries a *Preview* or *Experimental* badge (Supabase and
  PostHog [S7, H5]). Later, experimental features sit behind a per-project switch, as previews.

### 5.10 What not to copy, and why

| Practice | Who does it | Why not for Colophon (now) |
|---|---|---|
| Launch weeks with livestreams, hackathons, meetups, tickets, giveaways | Supabase, Appwrite | Need a team and an audience; a one-person version is Proof Week |
| Weekly releases | Strapi, Payload, Sanity | Release when a milestone's proof gate passes, plus patch releases for security |
| Canny or Productlane boards | Strapi, Directus | Paid tools and another inbox. Directus left GitHub because it didn't fit *their* workflow; for one person, GitHub *is* the workflow |
| Discord | Convex, Sanity, Appwrite | Real-time chat needs someone present; Sanity staffs it with a full-time support engineer. Discussions are async and searchable |
| Dated API versions | Sanity | Every live version is code to keep and secure |
| Response-format negotiation | Appwrite | Ship server, CLI and SDK in lockstep instead |
| Two maintained release lines | Appwrite, Payload | One maintainer can't backport across lines |
| "Not semver" | Directus | Honest, but it moves the work to every user; Colophon already committed to semver |
| Codemods | Strapi, Payload | They fix *user code*. Colophon users write little code against it (content types are data); a preflight check is worth more |
| An AI-curated changelog bot | PostHog | Built for 26 repositories; Colophon has one, and the AI pair already writes the entry in the branch |
| Release-PR automation (release-plz [G4]) | Rust crates | Built for publishing crates to crates.io; Colophon ships binaries and an HTTP API, and its changelog is written by hand in plain words |
| Monthly video show | Directus | Even Directus's has gaps; a short video per release is enough |

### 5.11 Documentation changes these recommendations would need (for the maintainer to decide)

If adopted, these are design changes to record in `DECISIONS.md` and the module docs, in the same change, per
`AGENTS.md`:

- **Module 19:** before 1.0, support only the latest minor and upgrades from the previous minor; the spec's
  previous-minor support with a 90-day transition starts at 1.0. Add the deprecation policy (5.5) and the release-notes
  template.
- **Module 04:** stability levels per contract; the definition of breaking; `Deprecation`, `Sunset` and 410 behaviour;
  `oasdiff` in CI; no version header on public delivery.
- **Module 08:** the update check and stderr deprecation warnings, both compatible with C1.
- **Module 16:** `/changelog` (with a feed), `/upgrading/0.N`, `/deprecations`, `/versioning`, and maturity badges
  (DS1).
- **Module 18:** the previous-minor upgrade path and the "are you affected?" checks.

---

## 6. Could not verify, and other caveats

- **Supabase:** the feature-stage definitions (seen only in a search summary; the page's `.md` returned 404); the year
  of Launch Week 15 and whether any later launch week exists (none found); the CLI update-notifier switch (seen in a
  third-party mirror of the release notes, not on GitHub); Discord, X and YouTube presence.
- **Appwrite:** Init 2026's exact dates (search summary); any current public roadmap (`/roadmap` returned 404); a
  Console "what's new" feature.
- **Convex:** Vibe Week (page timed out; search summary only; year unconfirmed); any CLI update notice (none found);
  an RSS feed for `ship.convex.dev` (none seen).
- **Strapi:** the Strapi 5 Launch Week day-by-day list (search summary); an admin-panel update banner (only the
  update-notification logging switch is confirmed); the community page doesn't list a forum or community calls, but I
  didn't check whether they exist elsewhere.
- **Sanity:** the changelog's RSS and JSON feed URLs and the Studio v2 end-of-support date (search summaries).
- **Payload:** the 4.0 beta announcement and the 2022 "roadmap released" post (search summaries); any in-product
  update notice; the community page returned 404.
- **Directus:** Leap Week details, the self-hosted upgrade page and the Cloud notice policy (search summaries); in-app
  notices.
- **PostHog:** early-access mechanics beyond the handbook (search summary); the live roadmap page didn't render for the
  fetch tool; its email page gives two cadences (monthly and bi-weekly).
- **Linear:** no feed seen; cadence inferred from six dated entries.
- **Status pages** other than Supabase's were not checked. They matter little for Colophon, which has no hosted service.
- **Tools named in recommendations:** `oasdiff`'s `--fail-on` flag and its deprecation features were not on the page I
  read; whether `utoipa` supports vendor extensions on operations was not checked; RFC 8594 (Sunset) was not fetched
  directly, only cited from RFC 9745.
- **Method:** pages were read through a summarising fetch tool, so wording may differ slightly from the source. Counts
  such as "8 entries between two dates" come from what the tool returned on 2026-10-27 and will change.

---

## 7. Sources (all checked 2026-10-27)

**Supabase**
- [S1] Changelog — https://supabase.com/changelog
- [S2] Changelog, breaking changes — https://supabase.com/changelog?tags=breaking-change
- [S3] GitHub Discussions, Changelog category (Atom feed in its description) — https://github.com/orgs/supabase/discussions/categories/changelog
- [S4] Launch Week — https://supabase.com/launch-week
- [S5] Self-hosting with Docker (updating) — https://supabase.com/docs/guides/self-hosting/docker
- [S6] Blog (RSS, newsletter signup) — https://supabase.com/blog
- [S7] Features and stage labels — https://supabase.com/docs/guides/getting-started/features (definitions search-level: https://supabase.com/docs/guides/getting-started/features.md)
- [S8] GitHub Discussions categories — https://github.com/orgs/supabase/discussions
- [S9] CLI v2.115.0 release notes, third-party mirror — https://newreleases.io/project/github/supabase/cli/release/v2.115.0
- [S10] Developer Update, July 2026 — https://supabase.com/changelog/47796-developer-update-july-2026 and https://github.com/orgs/supabase/discussions/47796
- [S11] Feature previews (search-level) — https://supabase.com/blog/tabs-dashboard-updates

**Appwrite**
- [A1] Changelog — https://appwrite.io/changelog
- [A2] Init 2026 recap — https://appwrite.io/blog/post/appwrite-init-2026-recap
- [A3] REST API docs — https://appwrite.io/docs/apis/rest
- [A4] Release policy — https://appwrite.io/docs/advanced/platform/release-policy
- [A5] Self-hosting updates — https://appwrite.io/docs/advanced/self-hosting/production/updates
- [A6] Community — https://appwrite.io/community
- [A7] Changelog entry, object arguments — https://appwrite.io/changelog/entry/2025-09-02

**Convex**
- [C1] Changelog and request board — https://ship.convex.dev/
- [C2] Release entry v1.42.0 — https://ship.convex.dev/changelog/convex-1-42-0
- [C3] npm package changelog — https://github.com/get-convex/convex-js/blob/main/CHANGELOG.md (copy read from https://cdn.jsdelivr.net/npm/convex@latest/CHANGELOG.md)
- [C4] Status and stability — https://docs.convex.dev/production/state
- [C5] Self-hosted README — https://github.com/get-convex/convex-backend/blob/main/self-hosted/README.md
- [C6] Self-hosted upgrading — https://github.com/get-convex/convex-backend/blob/main/self-hosted/advanced/upgrading.md
- [C7] Vibe Week (search-level) — https://www.convex.dev/vibe-week
- [C8] Community link (redirects to Discord) — https://www.convex.dev/community

**Strapi**
- [T1] GitHub releases — https://github.com/strapi/strapi/releases
- [T2] Upgrade tool — https://docs.strapi.io/cms/upgrade-tool
- [T3] v4 to v5 introduction and FAQ — https://docs.strapi.io/cms/migration/v4-to-v5/introduction-and-faq
- [T4] Breaking-change page example — https://docs.strapi.io/cms/migration/v4-to-v5/breaking-changes/removed-support-for-some-env-options
- [T5] Security policy — https://github.com/strapi/strapi/security/policy
- [T6] Feedback board and v4 notice — https://feedback.strapi.io/ and https://feedback.strapi.io/changelog/notice-strapi-v4-bug-support-coverage-is-ending-in-october
- [T7] Docs release notes — https://docs.strapi.io/release-notes
- [T8] Release roundup, Jan–Feb 2026 — https://strapi.io/blog/strapi-release-roundup-everything-that-changed-between-january-and-february-2026
- [T9] Strapi 5 Launch Week (search-level) — https://strapi.io/join-launch-week and https://strapi.io/blog/multi-environment-strapi-cloud
- [T10] Community — https://strapi.io/community
- [T11] RFC repository — https://github.com/strapi/rfcs

**Sanity**
- [N1] Changelog — https://www.sanity.io/changelog
- [N2] API versioning — https://www.sanity.io/docs/api-versioning
- [N3] Latest version of Sanity (channels, auto-updates) — https://www.sanity.io/docs/studio/latest-version-of-sanity
- [N4] Studio v6 announcement — https://www.sanity.io/blog/sanity-studio-v6
- [N5] Changelog entry v6.10.1 — https://www.sanity.io/docs/changelog/studio-Ni4xMC4w
- [N6] Slack to Discord — https://www.sanity.io/blog/we-just-deleted-our-35k-member-community-slack
- [N7] Everything NYC 2026 recap — https://www.sanity.io/everything-2026-recap
- [N8] Docs changelog and feeds (search-level) — https://www.sanity.io/docs/changelog
- [N9] Upgrade packages — https://www.sanity.io/docs/help/upgrade-packages
- [N10] Studio v2 support end: a search summary quoting Sanity's v2 changelog and v3 blog post; no single page opened

**Payload**
- [P1] GitHub releases — https://github.com/payloadcms/payload/releases
- [P2] Versioning — https://github.com/payloadcms/payload/blob/main/docs/migration-guide/versioning.mdx
- [P3] 3.0 to 4.0 migration guide — https://github.com/payloadcms/payload/blob/main/docs/migration-guide/v4.mdx
- [P4] Security update for 3.x and 4.0 — https://payloadcms.com/posts/blog/payload-security-update-available-for-3x-and-40
- [P5] GitHub Discussions — https://github.com/payloadcms/payload/discussions
- [P6] Launch Week day 2 (2022) — https://payloadcms.com/blog/launch-week-day-2
- [P7] 4.0 beta announcement (search-level) — https://payloadcms.com/posts/releases/payload-4-beta-redesigned-admin-tanstack-mcp
- [P8] Roadmap released (search-level) — https://payloadcms.com/blog/roadmap-released

**Directus**
- [D1] Releases (versioning statement) — https://directus.com/docs/releases
- [D2] Breaking changes, Version 12 — https://directus.com/docs/releases/breaking-changes/version-12 (index: https://directus.com/docs/releases/breaking-changes)
- [D3] GitHub releases and Atom feed — https://github.com/directus/directus/releases and https://github.com/directus/directus/releases.atom
- [D4] The Changelog show — https://directus.com/tv/the-changelog
- [D5] Feature requests — https://directus.com/docs/raw/community/contribution/feature-requests.md and https://roadmap.directus.com (move explained, search-level: https://directus.com/tv/leap-week/what-next-for-directus)
- [D6] Leap Week (search-level) — https://directus.com/resources/leap-week-1-announcements and https://directus.io/blog/everything-announced-leap-week-02
- [D7] Upgrading and Cloud upgrades (search-level) — https://directus.com/docs/self-hosting/upgrading and https://directus.io/docs/cloud/projects/version-upgrades

**PostHog**
- [H1] Changelog (RSS: https://posthog.com/changelog.rss) — https://posthog.com/changelog
- [H2] How to publish the changelog — https://posthog.com/handbook/wizard-and-docs/how-to-publish-changelog
- [H3] Email communications — https://posthog.com/handbook/marketing/email-comms
- [H4] Managing the roadmap — https://posthog.com/handbook/engineering/posthog-com/roadmap
- [H5] Releasing new products and features — https://posthog.com/handbook/product/releasing-new-products-and-features
- [H6] Early access feature management (search-level) — https://posthog.com/docs/feature-flags/early-access-feature-management

**Linear**
- [L1] Changelog — https://linear.app/changelog

**Standards and tools**
- [G1] Keep a Changelog 1.1.0 — https://keepachangelog.com/en/1.1.0/
- [G2] Semantic Versioning 2.0.0 — https://semver.org/
- [G3] RFC 9745, The Deprecation HTTP Response Header Field (March 2025) — https://www.rfc-editor.org/rfc/rfc9745.html (RFC 8594, Sunset, cited from it: https://www.rfc-editor.org/rfc/rfc8594.html)
- [G4] release-plz (Rust release PRs, git-cliff changelogs, cargo-semver-checks; considered, not recommended for an HTTP API) — https://github.com/release-plz/release-plz
- [G5] oasdiff — https://github.com/oasdiff/oasdiff
- [G6] GitHub Discussions categories (Announcement format: "only people with maintain or admin permissions can create new discussions") — https://docs.github.com/en/discussions/managing-discussions-for-your-community/managing-categories-for-discussions
- [G7] Show HN guidelines — https://news.ycombinator.com/showhn.html

**Colophon (local, read only)**
- `projects/colophon-3.5/`: `AGENTS.md`, `Cargo.toml`, `docs/END_GOAL.md`, `docs/ROADMAP.md`, `docs/DECISIONS.md`,
  `docs/modules/04-api.md`, `08-cli.md`, `16-docs-site.md`, `18-operations.md`, `19-oss.md`
- `projects/colophon-site/` (branch `site/website`): `CHANGELOG.md`, `apps/site/src/lib/server/changelog.ts`,
  `apps/site/src/routes/{changelog,roadmap,blog}`
- `projects/colophon-3.6/` (branch `p3/3.6-modelling`): `docs/DECISIONS.md` (D-057 to D-059), `docs/COMPARISON.md`
- `minimal-portfolio/content/colophon/` (build log front matter); `x-growth/README.md` (posting routine and guardrails)
