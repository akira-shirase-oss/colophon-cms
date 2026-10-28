<script>
	import Status from '#lib/components/Status.svelte';
</script>

Short answers to the questions people ask first, each with a link to the long one.

## Is it ready for production?

No. Colophon is pre-release: v0.1 is not out, there is no installer, no hosted service, no backup or restore procedure <Status planned="P6" />, no confirmed licence and no security policy. What exists runs on a developer's machine against a local stack whose passwords are public. v0.1 is the end of milestone P6, after a 24-hour soak, a restore drill and an install by someone other than the maintainer; the roadmap estimates 9 to 12 months in all, as a guess, not a promise. [Roadmap and status](/resources/roadmap/) has every step.

## What's the licence?

Not confirmed yet. Apache-2.0 is proposed for the core, the command line and the SDK, and the maintainer must approve it before the first public release (open decision O-014). Until then the repository has no `LICENSE` file, which means no licence has been granted: read the code, but don't build on it yet.

## How is it different from Strapi or Sanity?

Since 2026-10-27, Colophon's goal is every feature Strapi and Sanity offer (decision D-057), so the difference isn't meant to be a shorter feature list. It is how each feature is built and what it promises:

- every save is a version that never changes, and a save made on an old version is refused, never merged silently;
- an approval counts only for the exact versions it was given on;
- every publish is a numbered release, which can be read later exactly as it was;
- each release ends with a receipt that says where it was actually seen, and never claims more;
- a feature counts as done only with its failure tests, a testbed page and a load scenario.

Today Colophon has far fewer features than either. The [comparison on the website](site:/compare/) is the short version; the full one, with the parity backlog, is [COMPARISON.md](/design/comparison/).

## Does it have GraphQL?

Not yet. GraphQL is planned, over the same queries and permissions as a new query API, with the TypeScript SDK and import from Strapi and Sanity. <Status planned="D" /> Today there is a REST management API under `/api/v1` and a public, read-only delivery API under `/content/v1` ([reference](/reference/)).

## Can I add images and other media?

Not yet. Uploads, the media library and images in rich text come with milestone P4, with checks on every upload. <Status planned="P4" /> Until then an image in rich text shows as a placeholder.

## Does it do localization? Scheduled publishing?

Localization isn't built. Each locale will be a variant of one entry with its own URL name, drafts and publication history, and fallbacks between languages will be an explicit project setting, off by default. <Status planned="E" />

Yes: an approved publish request can be scheduled for a date and time in a time zone you pick. <Status done="D-060" /> A schedule stores the local time you chose, its IANA time zone, the exact moment that resolves to, and the approval it relies on; a time that daylight saving makes ambiguous or skips must be chosen explicitly. When it is due, the permissions, the approval and the exact versions are checked again, so content edited since approval blocks the schedule instead of quietly changing what goes out. It runs as the person who scheduled it, with their role as it is then. A schedule that runs late is labelled *delayed*, and cancelling one stops it from starting but doesn't undo a release already made. [Scheduled publishing](/concepts/scheduled-publishing/) has the details.

## Can I host it myself?

That is the plan: Colophon is meant to be self-hosted. Today, only for development and evaluation. The server needs PostgreSQL, the only place data lives, and an OpenID Connect sign-in provider so people can sign in; a Valkey cache is optional, and losing it loses nothing. The local stack runs all of them in Docker (PostgreSQL 18, Valkey, Keycloak), plus S3-compatible storage that the media library will use later. [Self-hosting requirements](/operations/requirements/) lists what each is for and what isn't ready for production yet.

## Which sign-in providers work?

Colophon signs people in through OpenID Connect, with discovery, the Authorization Code flow with PKCE, and its own checks of every ID token. It is tested only with Keycloak, which the local stack runs. It has no passwords of its own. [Sign-in and sessions](/reference/api/auth/) has the details.

## What does "Verified at 1 location" mean?

A probe, a website connection that checks the site, fetched a page after the release and read that it was built from this release or a later one, from one place: the machine the worker runs on. It proves what that place saw at that moment, nothing more; it doesn't mean every reader everywhere sees the release. With probes in more places, the number grows. [Receipts: is it on the website?](/editors/receipts/) explains every label, and [Receipts and probes](/concepts/receipts-and-probes/) the rules.

## Where do the load numbers come from?

One laptop: an Apple M2 Pro with 12 cores and 32 GB of memory, running macOS, with the server's release build, the local stack and the k6 load generator all on the same machine (`scripts/load.sh`). The runs date from 2026-10-25 to 2026-10-27, and each result file in `perf/results/` names the machine, the commit, the date and the scenario. A throughput run sends a million requests, and a run that misses its thresholds or regresses against the machine's baseline fails. They are evidence about the code on that machine, never a statement of production capacity. [Performance](/resources/performance/) shows every result.

## How do I report a security issue?

There is no `SECURITY.md` and no private reporting channel described in the repository yet. A security policy with private advisories through GitHub, and a table of supported versions, is planned for v0.1 ([the release plan](/design/releases/)). Until then, don't post details of a vulnerability in a public issue: open an issue that asks the maintainer for a private way to send it, without the details. Remember that nothing here should be exposed to untrusted users yet. The [security model](/resources/security/) says what is protected today and what isn't.

## Where's the changelog?

The [changelog](/changelog/) lists every step merged into `main`, by milestone, and what is being built on branches; it is rendered from `CHANGELOG.md` in the repository. [Release notes](/releases/) summarise milestones P0 to P3, and [upgrade guides](/upgrading/) cover each range of database migrations. There are no version numbers yet: they start at v0.1.

## How is it built?

In the open, by one maintainer pairing with Claude Code, one tested step at a time. Every feature ships with regression tests that CI runs on every push, coverage in a separate testbed app that drives the real API, and a load scenario with thresholds, on the same branch. Database migrations, sign-in, publishing transitions and adapters to outside services get a line-by-line human review. Every step, mistakes included, is written up in the build log at [404human.dev/colophon](https://404human.dev/colophon/).
