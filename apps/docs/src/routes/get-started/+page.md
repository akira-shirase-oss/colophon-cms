<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon is an open-source content management system with a Rust server and a Svelte web app. Editors write, review and publish in the **Studio**; developers use an HTTP API and a command line; websites read published content through a public, read-only API. Its promise is that it can prove what it published: every publish is a numbered **release**, and every release ends with a **receipt** of where it was actually seen.

In a book, the *colophon* is the note at the back recording how, when and by whom it was printed. Colophon keeps that record for a website.

## Pre-release: read this first

Colophon is **pre-release**. Version 0.1 is not out, and nothing here is ready for production use:

- there is no installer, no hosted service and no production guide; the [local stack](/operations/local-stack/) is for development, and its passwords are public;
- the licence isn't confirmed: Apache-2.0 is proposed for the core, the CLI and the SDK, and the maintainer confirms it before any release (open decision O-014);
- there is no backup or restore procedure yet <Status planned="P6" />, and no security policy file yet ([FAQ](/resources/faq/#how-do-i-report-a-security-issue)).

It is built in the open by one maintainer pairing with Claude Code, one tested step at a time, and written up at [404human.dev/colophon](https://404human.dev/colophon/).

## What the labels mean

Pages and features in these docs carry one of three labels, the same as on the website; pages that only introduce others, like this one, carry none of their own:

| Label | Meaning |
|---|---|
| **Done** | Merged into `main`, with its regression tests, its page in the testbed app and its load scenario. |
| **Building** | A step in progress on its own branch. Not merged; it can still change. |
| **Planned** | In the roadmap. Nothing is built yet. |

A Building or Planned feature is never described as something you can use today. [Roadmap and status](/resources/roadmap/) lists every step.

## What you can do with it today

On your own machine, against the local stack:

- **Write** drafts that save themselves, survive a closed tab or a dropped network, and never silently overwrite a colleague's save. <Status done="1.3" />
- **Compare versions** of an entry word by word, and find entries by title. <Status done="3.5" />
- **Set it up from the browser**: create an account, enter the one-time code the server printed, and start a project as a small blog, a documentation site or blank, with a checklist for what to do first. <Status done="3.7" />
- **Define content types** in the Studio's builder or through the API, with text, rich text, number, yes/no, choice, date-time and URL-name fields, changed only in ways that keep every stored entry valid. <Status done="3.6" />
- **Invite people** with single-use links, into one of four built-in roles or roles owners define, down to some fields or a member's own entries. <Status done="R.3" />
- **Review exact changes.** A publish request pins the versions in it, and an approval counts only for those exact versions. <Status done="2.1" />
- **Publish** an approved request as one release, which the public API serves at once and can still serve later exactly as it was <Status done="2.2" />, in one click from an entry when the project lets you approve your own <Status done="3.7" />, or at a time you choose, in your time zone. <Status done="D-060" />
- **Serve readers** from a cache keyed by release, which can't serve an answer from the wrong one, with ETags and CDN cache headers. <Status done="3.1" />
- **Build a website** from exactly one release, with the reference site in `apps/web`, which carries its release number on every page. <Status done="3.4" />
- **Tell your website**: webhooks run from a worker that survives being killed <Status done="2.3" />, CDN purges <Status done="3.2" />, and probes that check the site, so each release gets one honest label. <Status done="3.3" />
- **Take something down** through review, so Colophon's own server never serves it again, at any past release either. <Status done="3.2" />
- **Use the command line** to sign in, create and list entries <Status done="1.6" />, and, as the operator, apply migrations and make the first administrator. <Status done="1.8" />

Still to come, among others:

- API tokens and an audit log, built on a branch <Status building="R.4" />;
- the site going live, and the first public demo <Status planned="P3" />;
- relations, reusable groups of fields and lists <Status planned="M" />;
- media and images, preview links, the release slider and rollback <Status planned="P4" />;
- localization, review stages, comments and live collaboration <Status planned="E" />;
- a query API, GraphQL, the SDK and import from Strapi and Sanity <Status planned="D" />;
- several servers behind a load balancer, a stress run for every load scenario, and Rust benchmarks <Status planned="S" />;
- one version across the server, the command line, the SDK and the MCP server, installable from npm, Cargo and Homebrew <Status planned="V" />;
- scoped credentials for scripts and agents <Status planned="P5" />;
- backups, restore and the v0.1 release <Status planned="P6" />.

## Who it is for

- **Editors** write, review and publish in the Studio, without a terminal and without needing to know what a cache is. Start with the [editor guide](/editors/).
- **Developers** run it locally, define content types and use the APIs. Start with [Run it locally](/get-started/quickstart/), then the [reference](/reference/).
- **Operators** run the server, the worker and their dependencies. Start with [Self-hosting requirements](/operations/requirements/), and read the pre-release warning above twice.
- **Anyone curious about the engineering**: the [concepts](/concepts/) explain each guarantee and the test behind it, and the [design documents](/design/) are the plan itself.

## How these docs are organised

| Section | What is in it |
|---|---|
| [Get started](/get-started/) | This page, then four short walkthroughs: [run it locally](/get-started/quickstart/), [your first project](/get-started/first-project/), [your first article](/get-started/first-article/) and [your first publish and receipt](/get-started/first-publish/). |
| [For editors](/editors/) | The Studio in plain words: writing, versions, URL names, review, publishing, receipts and taking something down. |
| [Concepts](/concepts/) | One idea per page: what Colophon promises, how it keeps the promise, and which test proves it. |
| [Developer reference](/reference/) | Every API route, error code, CLI command, the rich-text format and webhooks. |
| [Operations](/operations/) | Requirements, the local stack, configuration, health checks, the worker, migrations and crash tests. |
| Resources | The [security model](/resources/security/), [performance results](/resources/performance/), the [roadmap](/resources/roadmap/), the [FAQ](/resources/faq/) and the [glossary](/resources/glossary/). |
| Updates | The [changelog](/changelog/), [release notes](/releases/) per milestone and [upgrade guides](/upgrading/). |
| [Design documents](/design/) | The architecture, decisions, roadmap and module plans, rendered from the repository. |

The editor pages use plain words (a *publish request*, a *version*, a *URL name*); the developer pages use the API's terms. The [glossary](/resources/glossary/) puts them side by side.

## What it is not

Colophon isn't trying to be first, or fastest in a synthetic benchmark, or "AI-powered". Since 2026-10-27 its goal is every feature Strapi and Sanity offer, each built with the same proof (decision D-057 in the [decisions](/design/decisions/)). Two things stay out on purpose: native plugins from strangers running inside the server, and commerce ([the parked list](/design/modules/parked/)). Load-test numbers in these docs come from one laptop, with its name and the date; they are evidence about the code, never a claim about production capacity.
