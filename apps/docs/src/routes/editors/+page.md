<script>
	import Status from '#lib/components/Status.svelte';
</script>

This guide is for the people who write and publish: no terminal, no database, nothing about caches. Everything it describes happens in the **Studio**, Colophon's web app, at `/studio/` on your Colophon server.

## The path, in four steps

1. **Write.** Open an entry and type. The Studio saves a second after you stop, and keeps a copy on your device until the server has it. [Writing and autosave](/editors/writing/)
2. **Ask for review.** Put the entries you want to publish into a publish request and send it for review. The reviewer approves exactly what you sent. [Asking for review](/editors/review/)
3. **Publish.** An approved request becomes one numbered release: all of it goes live together, or none of it does. [Publishing](/editors/publishing/)
4. **Check the website.** Each release ends with one label: *Published in CMS*, *Website update pending*, *Verified at 1 location* or *Needs attention*. [What the website labels mean](/editors/website-status/)

Today a publish request is always reviewed before it is published. Publishing a single entry with one button on the entry is planned for step 3.7. <Status planned="P3" />

## Words used in this guide

These guides use plain words, which the maintainer adopted on 2026-10-27 (D-059). The Studio is being changed to use them too; until that change lands (step 3.7), some screens still show the older word in the middle column.

| In this guide | On the Studio's screens today | In the API and the command line |
|---|---|---|
| publish request | Changeset | changeset |
| version | Revision, in an entry's history | revision |
| release | Release | release, numbered by its generation |
| URL name | not there yet: it arrives with step 3.6 | slug |
| website connections | Destinations, under Settings | destinations: webhooks, CDN purges and probes |
| collection | the list of entries of one type, in the sidebar | content type |

## What you can't do yet

Being honest about the gaps matters more than a long list. Not built yet: images and media <Status planned="P4" />, preview links <Status planned="P4" />, scheduled publishing <Status planned="P4" />, restoring an old version as a new one <Status planned="P4" />, the release slider that shows the site as it was <Status planned="P4" />, and editing two people at once in one entry, which Colophon deliberately doesn't merge: it shows you both versions instead.
