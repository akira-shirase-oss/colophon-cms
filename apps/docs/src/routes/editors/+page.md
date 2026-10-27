<script>
	import Status from '#lib/components/Status.svelte';
</script>

This guide is for the people who write and publish: no terminal, no database, nothing about caches. Everything it describes happens in the **Studio**, Colophon's web app, at `/studio/` on your Colophon server. Each page ends with a short *Technical details* section for developers; you can skip it.

## The path, in four steps

1. **Write.** Open an article and type. The Studio saves a second after you stop, and keeps a copy on your device until the server has it. [Writing and saving](/editors/writing/)
2. **Ask for review.** Put what you want to publish into a publish request and send it for review. The reviewer approves exactly what you sent. [Publish requests and review](/editors/review/)
3. **Publish.** An approved request becomes one numbered release: all of it goes live together, or none of it does. [Publishing](/editors/publishing/)
4. **Check the website.** Each release ends with one label: *Published in CMS*, *Website update pending*, *Verified at 1 location* or *Needs attention*. [Receipts: is it on the website?](/editors/receipts/)

Along the way: [joining a project](/editors/joining/), [versions and comparing them](/editors/versions/), [URL names](/editors/url-names/) <Status building="3.6" /> and [taking something down](/editors/taking-down/).

Today every publish request is reviewed before it is published. Publishing one article with a single button on the article is being built. <Status building="3.7" />

## Words used in this guide

These pages use plain words, which the maintainer adopted on 2026-10-27 (decision D-059). The Studio is being changed to use them too, in step 3.7 <Status building="3.7" />; until then, some screens still show the older word in the right-hand column.

| In this guide | On the Studio's screens today |
|---|---|
| publish request | Changeset |
| version | Revision, in an entry's side panel and when comparing |
| release, and the time it went live | Release, shown as *No. 3* |
| URL name | not there yet: it arrives with step 3.6, where a few messages still say *slug* |
| website connections: rebuild the site, clear the CDN, check the site | Destinations, under Settings: Webhook, CDN and Probe |
| article, or entry | Entry |
| collection | the list of entries of one type, under *Content* in the sidebar |

The [glossary](/resources/glossary/) lists every term with its plain word.

## What you can't do yet

Being clear about the gaps matters more than a long list. Not built yet:

- images and other media in articles <Status planned="P4" />;
- preview links that show a draft as the website would <Status planned="P4" />;
- scheduled publishing <Status building="e/schedules" />;
- restoring an old version as a new one, and the release slider that shows the site as it was <Status planned="P4" />;
- comments, review stages and two people editing one article at the same moment <Status planned="E" />. Today Colophon never merges two people's edits behind their backs: it shows you both versions and lets you choose.

## Technical details

The API and the command line use the older, technical words: a publish request is a `changeset`, a version a `revision`, a URL name a `slug`, a release is numbered by its `generation`, and website connections are `destinations` of three kinds, `webhook`, `cdn` and `probe`. The [developer reference](/reference/) uses those words throughout.
