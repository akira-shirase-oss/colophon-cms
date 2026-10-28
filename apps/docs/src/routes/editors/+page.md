<script>
	import Status from '#lib/components/Status.svelte';
</script>

This guide is for the people who write and publish: no terminal, no database, nothing about caches. Everything it describes happens in the **Studio**, Colophon's web app, at `/studio/` on your Colophon server. Each page ends with a short *Technical details* section for developers; you can skip it.

## The path, in four steps

1. **Write.** Open an article and type. The Studio saves a second after you stop, and keeps a copy on your device until the server has it. [Writing and saving](/editors/writing/)
2. **Ask for review.** Press **Ask for review** on the article, or gather several articles into one publish request. The reviewer approves exactly what you sent. [Publish requests and review](/editors/review/)
3. **Publish.** An approved request becomes one numbered release: all of it goes live together, or none of it does. If your project lets you approve your own work and your role may publish, the article's **Publish** button does steps 2 and 3 in one click. [Publishing](/editors/publishing/)
4. **Check the website.** Each release ends with one label: *Published in CMS*, *Website update pending*, *Verified at 1 location* or *Needs attention*. [Receipts: is it on the website?](/editors/receipts/)

Along the way: [joining a project](/editors/joining/), [versions and comparing them](/editors/versions/), [URL names](/editors/url-names/) and [taking something down](/editors/taking-down/).

A new project's overview starts with a short checklist: write the first article, publish it (or ask for review), connect the website, invite a teammate, and see the first receipt. Each item ticks itself off from what really happened, and items your role can't do are left out. <Status done="3.7" />

## Words used in this guide

These pages and the Studio use the same plain words (decision D-059, applied to every Studio screen in step 3.7). <Status done="3.7" /> The API, the command line and the Studio's *Technical details* drawers keep the technical ones:

| In this guide and the Studio | In the API and the command line |
|---|---|
| publish request | changeset |
| version | revision |
| release, shown as *Release 3* with the time it went live (*No. 3* in the list of releases) | generation |
| URL name | slug |
| website connections: *Site rebuild*, *CDN clear* and *Site check* | destinations: `webhook`, `cdn` and `probe` |
| article, or entry | entry |
| collection: the entries of one content type, under *Content* in the sidebar | collection |

The [glossary](/resources/glossary/) lists every term with its plain word.

## What you can't do yet

Being clear about the gaps matters more than a long list. Not built yet:

- images and other media in articles <Status planned="P4" />;
- preview links that show a draft as the website would <Status planned="P4" />;
- restoring an old version as a new one, and the release slider that shows the site as it was <Status planned="P4" />;
- comments, review stages and two people editing one article at the same moment <Status planned="E" />. Today Colophon never merges two people's edits behind their backs: it shows you both versions and lets you choose;
- reading and reviewing on a phone: the Studio is checked at phone width on two screens so far (the list of publish requests and one request), not on the others. <Status planned="3.7" />

## Technical details

The API and the command line use the older, technical words: a publish request is a `changeset`, a version a `revision`, a URL name a `slug`, a release is numbered by its `generation`, and website connections are `destinations` of three kinds, `webhook`, `cdn` and `probe`. The [developer reference](/reference/) uses those words throughout.
