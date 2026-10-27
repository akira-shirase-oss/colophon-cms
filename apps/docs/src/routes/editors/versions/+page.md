<script>
	import Status from '#lib/components/Status.svelte';
</script>

Every save makes a new **version** of the entry. Versions are never edited or deleted afterwards, so the history of an entry is complete: every save, from the Studio, the command line or a script, is in it.

## Looking at the history

Open an entry and choose the **History** tab. It lists every version, newest first, with its title, when it was saved and a short fingerprint of its content (the first characters of its SHA-256 hash). Two badges help you find your way:

- **Current draft** marks the newest version, the one you are editing.
- **Live in No. N** marks the version readers see, and the release that published it.

## Comparing a version with your draft

Press **Compare with draft** on any older version. The Studio shows what changed between that version and the current draft, word by word, field by field. Press **Hide** to fold it away.

The same comparison appears when you review a publish request: for every entry in it, you see what changes for readers, from the version that is live now to the one being published. New entries show as all new; an entry being unpublished shows what goes.

## Going back to an older version

Restoring an older version, as a new version on top of the draft, isn't built yet. <Status planned="P4" /> Until then, you can copy what you need from the comparison into the draft.

Seeing the whole website as it was at an earlier release, with a release slider, also comes with milestone P4. <Status planned="P4" /> The data for it is already kept: every release's content can be read exactly as it was through the delivery API.
