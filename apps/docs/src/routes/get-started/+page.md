<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon is an open-source content management system with a Rust server and Svelte frontends. Editors write, review and publish in a web app called the Studio; developers use a typed HTTP API and a command line; websites read published content through a public, read-only API. What sets it apart is not a feature list. It is that every publish is a numbered **release**, and every release ends with stored evidence of where it was actually seen.

In a book, the *colophon* is the note at the back recording how, when and by whom it was printed. Colophon keeps that record for a website.

## Where it stands

Colophon is **pre-release**. Version 0.1 is not out, and nothing here is ready for production use: there is no hosted service, no installer and no confirmed licence yet (Apache-2.0 is proposed, decision O-014). The project is built in the open by one maintainer pairing with Claude Code, one tested step at a time, and logged at [404human.dev/colophon](https://404human.dev/colophon/).

Milestones P0 (foundation), P1 (never lose an edit) and P2 (publish exactly once) are done. P3 (prove it reached the site) is in progress. [Status and roadmap](/get-started/status/) has the detail.

## What you can do with it today

On your own machine, against the local stack:

- **Write** drafts that autosave, survive a closed tab or a dropped network, and never silently overwrite a colleague. <Status implemented />
- **Define content types** with text, rich text, number, yes/no, choice and date-time fields, changed only in ways that keep every stored entry valid. <Status implemented />
- **Review exact changes.** A publish request pins the versions in it, and an approval counts only for those exact bytes. <Status implemented />
- **Publish** an approved request as one release, which the public API serves at once and can serve later exactly as it was. <Status implemented />
- **Tell your website**: webhooks, CDN purges and probes run from a worker that survives being killed, and each release gets one honest label. <Status implemented />
- **Unpublish** with review, so the origin never serves the entry again, at any past release either. <Status implemented />
- **Use the command line** to sign in, create and list entries, and run the operator's bootstrap. <Status implemented />

Still to come, among others: media uploads and images, preview links, scheduled publishing, the release slider and rollback <Status planned="P4" />; scoped tokens for scripts and agents <Status planned="P5" />; backups, restore and upgrades <Status planned="P6" />.

## Who it is for

- **Editors** write, review and publish in the Studio, without a terminal and without needing to know what a cache is. Start with the [editor guide](/editors/).
- **Developers** run it locally, define content types and use the APIs. Start with the [quickstart](/get-started/quickstart/), then the [reference](/reference/).
- **Anyone curious about the engineering**: the [concepts](/concepts/) explain each guarantee and how it is tested, and the [design documents](/design/) are the plan itself.

## What it is not

Colophon is not trying to be first, fastest in a synthetic benchmark, or "AI-powered". Commerce, payments and hosting integrations are deliberately parked ([the parked list](/design/modules/parked/)). Laptop benchmark numbers in these docs are evidence about the code on one machine, never a claim about production capacity.
