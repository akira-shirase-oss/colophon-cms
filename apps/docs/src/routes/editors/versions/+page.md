<script>
	import Status from '#lib/components/Status.svelte';
</script>

Every save makes a new **version** of the article. Versions are never edited or deleted afterwards, so an article's history is complete: every save, from the Studio, the command line or a script, is in it.

## Looking at the history

Open an article and choose the **History** tab. It lists the versions, newest first (the newest 100; older ones are kept, but the Studio doesn't page back to them yet), each with its title, when it was saved and a short fingerprint of its content (the first characters of a SHA-256 hash, the same for two versions only when their content is identical). Two badges help you find your way:

- **Current draft** marks the newest version, the one you are editing.
- **Live in No. N** marks the version readers see, and the release that published it.

## Comparing a version with your draft

Press **Compare with draft** on any older version. The Studio shows what changed between that version (it says *this revision*, the older word) and the current draft, word by word, field by field. Press **Hide** to fold it away.

The same comparison appears when you review a publish request: for every article in it, you see what changes for readers, from the version that is live now to the one being published. A new article shows as all new; one being taken down shows what goes.

## Going back to an older version

Restoring an older version, as a new version on top of the draft, isn't built yet. <Status planned="P4" /> Until then, copy what you need from the comparison into the draft; your save becomes a new version, and the old one stays in the history.

Seeing the whole website as it was at an earlier release, with a release slider, also comes later. <Status planned="P4" /> What it needs is already kept: every release's content can still be read exactly as it was.

## Technical details

A version is a `revision`: `GET …/entries/{entry}/revisions` lists the newest 100 (there is no cursor for older ones yet) and `…/revisions/{revision}` reads any one, under the project's `/api/v1/projects/{project}/environments/production/collections/{collection}` path. Revisions are insert-only rows, and an entry's draft is a pointer to its latest one. Published versions are read with `?generation=N` on the delivery API. [Drafts and versions](/concepts/drafts-and-versions/) and [Entries and versions](/reference/api/entries/) have the rest.
