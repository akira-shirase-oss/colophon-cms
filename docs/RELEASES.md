# Releases: how Colophon tells people what changed

How Colophon keeps its users up to date, sized for one maintainer and an AI pair. It adopts the recommendations of
[research/2026-10-27-keeping-users-informed.md](research/2026-10-27-keeping-users-informed.md), which studied Supabase,
Appwrite, Convex, Strapi, Sanity, Payload, Directus and PostHog from their own pages (sources and what couldn't be
verified are listed there).

## Principles

- **Proof, not adjectives.** An entry that makes a guarantee names the test or load result behind it.
- **Plain words first** ([D-059](DECISIONS.md)): the first line of every entry is for an editor (a *publish request*,
  a *version*, a *URL name*); routes, codes and decision numbers follow.
- **One source per fact.** The changelog is `CHANGELOG.md`, the roadmap `ROADMAP.md`, decisions `DECISIONS.md`.
  The website and docs render them; nothing is typed twice.
- **Written with the change.** The branch that changes behaviour adds its changelog entry, as it already updates the
  module docs.
- **Promise only what one person can keep:** few promises, each dated.

## Now (pre-release)

1. `CHANGELOG.md` at the repository root, one entry per merged step, newest first; the website renders it at
   `/changelog` with a feed. Each step's merge adds its entry; a malformed entry fails the site's build.
2. Capabilities are labelled **Stable**, **Preview**, **Experimental** or **Planned** on the website and in the docs.
   Before v0.1, nothing is Stable except the `colophon/rich-text` v1 format, which is permanent.
3. GitHub Discussions with four categories: Announcements (maintainers only; it has an Atom feed), Ideas (with votes),
   Q&A, and Show and tell. Ideas are triaged into `ROADMAP.md`; decided proposals become entries in `DECISIONS.md`.
   No Discord, Canny or community calls until there are people to talk to.
4. Each step still ends with a build-log entry at https://404human.dev/colophon/, which the website's blog mirrors.
5. The licence is settled (O-014) before anyone is invited to try Colophon.

## At v0.1

- **Versions.** `CHANGELOG.md` switches to version sections in the Keep a Changelog 1.1 format (Added, Changed,
  Deprecated, Removed, Fixed, Security), with the step history kept below. Every entry starts with its area (Studio,
  Site, Delivery, API, CLI, SDK, Worker, Operations, Docs) and says *Breaking* when it breaks something: old behaviour,
  new behaviour, what to do.
- **Release notes** from [templates/release-notes.md](templates/release-notes.md), published as the GitHub Release and
  on the website: upgrade at a glance, breaking changes, deprecations, highlights, all changes, known issues, the
  evidence that ran for the release, verification of the download, and a generated compatibility table.
- **Upgrade guide** per minor release from [templates/upgrade-guide.md](templates/upgrade-guide.md), at
  `docs/upgrading/0.N.md`: an "are you affected?" check, the steps, verification with Colophon's own receipts, and when
  rolling back is safe. Upgrades are supported from the previous minor; CI rehearses that upgrade.
- **Stability of each contract.**

  | Contract | From v0.1 |
  |---|---|
  | `/content/v1` (public delivery) | Stable: only ever gains things |
  | `/api/v1` (management) | Preview until 1.0; a break needs a *Breaking* entry |
  | `colophon/rich-text` v1 | Stable, permanently |
  | CLI `--json` results, webhook payloads | Stable (payloads carry a version field) |

  Breaking means removing or renaming a route, field, parameter or error code, changing a type or a meaning, making
  something optional required, adding a precondition to an existing route, or narrowing validation. New routes,
  optional parameters, response fields and headers are not breaking: clients ignore what they don't know.
- **Machine checks.** The OpenAPI export planned in D-027 is committed and `oasdiff breaking` runs in CI against the
  last release: a finding fails CI for `/content/v1` and requires a *Breaking* entry for `/api/v1`. CLI JSON and webhook
  payloads get golden-file tests.
- **In the product.** The Studio shows its version with a link to the changelog. The CLI checks for a newer release at
  most once a day, says so on stderr (never in `--json` mode), and can be switched off. The server never phones home.
- **Security.** `SECURITY.md` with private advisories through GitHub, and a table of supported versions.

## Deprecation policy

- **Before 1.0:** any minor release may break things, and every break is labelled and explained; patch releases never
  break anything; where keeping old behaviour one more minor is cheap, it stays and warns; data is never changed
  destructively without the operator acting; stored formats are permanent. A bad release stays listed, marked
  *Yanked*, with the release that fixes it.
- **From 1.0:** a deprecation is announced in a minor release with its replacement and removal date; it keeps working
  for at least one more minor release and 90 days, whichever is later, and is removed only in a major release. Routes
  that are going away answer with `Deprecation` and `Sunset` headers (RFC 9745, RFC 8594) and a link to the guide, and
  answer 410 once removed. Versions stay in the path (`/api/v1`, `/content/v1`): no dated API versions, no
  response-format header; a `v2` would run beside `v1` for a full cycle.

## Launches

- **The first public demo** (the step that puts the reference site live) is one launch day: a short recorded walk-through of the crash loop and a
  verified release, a build-log write-up, a pinned announcement, and a post. It recruits the independent installer
  and the five editors v0.1's definition of done needs. No Show HN until someone can install Colophon from the README.
- **v0.1** gets a "proof week": one technical write-up a day from the ones v0.1 already requires (the outbox, release
  generations, the simulator, cache invalidation), then the release itself.
