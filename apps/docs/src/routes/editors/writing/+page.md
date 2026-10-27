<script>
	import Status from '#lib/components/Status.svelte';
</script>

Open a collection from the sidebar (**Articles**, for example), then an article, or press **New entry**. A new article asks only for what it can't start without, usually its title, and starts as a draft: nothing is published until it has been reviewed and published. Each field has its own row; rich-text fields have a formatting toolbar for headings, lists, quotes, code, callouts and links. You don't save by hand: the Studio does it for you, and it tells you plainly where your words are.

## How saving works

- A second after you stop typing, the Studio saves. One save runs at a time; anything you type meanwhile goes in the next one.
- **Save now**, or ⌘S (Ctrl+S on Windows and Linux), saves at once.
- Before anything reaches the server, your edit is kept **on this device**, in this browser. If the tab closes, the network drops or the server restarts, nothing typed is lost: open the article again and it comes back, with *Restored unsaved changes from this device.*
- The device copy is removed as soon as the server has your edit, and expires after 30 days if it never gets there.
- A save that gets no answer is sent again as the same request, so the server can tell it is a repeat and never stores it twice.

## What the save message means

The message in the panel beside the fields is the one place that says where your work is. It says **Saved** only after the server has confirmed it.

| Message | What it means | What to do |
|---|---|---|
| Saved | The server has stored this version. | Nothing. |
| Saving… | A save is on its way. | Keep writing. |
| Unsaved changes, kept on this device | You have typed since the last save; the next save is about to start. | Nothing. |
| Can't reach the server. Your changes are kept on this device; retrying. | The network or the server is down. Your text is safe in this browser. | Keep the tab open, or come back later on this device. |
| Not saved: someone else saved first | Another person saved this article after you opened it. | Choose a version (below). |
| Not saved: followed by a reason | The server refused the content, for example a title that is too long. | Fix what the reason names. |
| Signed out: changes kept on this device | Your session ended. | Press **Sign in again**; your text is still here. |

Every request gives up after 15 seconds, so nothing spins forever: you always see one of the messages above.

## When two people edit the same article

Colophon never merges two people's edits behind their backs, and never lets a later save silently overwrite an earlier one. If someone saved the article after you opened it, your save is refused and the Studio shows both versions side by side, **Your version** and **Their version**:

- **Keep my version** saves yours on top of theirs. Theirs isn't lost: it stays in the article's history.
- **Use their version** discards your unsaved changes and shows theirs.

Until you choose, your text stays on your device. Editing one article together, with each other's cursors, isn't part of Colophon today. <Status planned="E" />

## Where the article stands

The panel beside the fields shows the article's status, the release it is live in, its current version (the Studio still labels it *Revision*), and when it was saved and created. In a collection's list, every article has one of three states, each with a word as well as a colour:

- **Draft**: never published, or taken down; readers can't see it.
- **Live**: readers see this version.
- **Changed**: it is live, and has been edited since; readers still see the older version until you publish again.

The list has **All**, **Drafts**, **Changed** and **Live** filters, and **Search titles** finds articles by title, beyond the first page.

## Not built yet

- Images in rich text show as placeholders until the media library exists. <Status planned="P4" />
- Preview links that show a draft as the website would. <Status planned="P4" />
- On a shared computer, the device copies of unsaved edits stay until they are saved or expire after 30 days; there is no sign-out button that clears them yet.

## Technical details

Each save is `PUT` on the entry with `If-Match` naming the version it builds on and an `Idempotency-Key` reused while the base and the content are unchanged; a stale base gets `409` with the reason `stale_revision`. Rich text is stored as validated JSON in the `colophon/rich-text` v1 format, never HTML. [Saves and conflicts](/concepts/saves-and-conflicts/), [Entries and versions](/reference/api/entries/) and [Rich-text format](/reference/rich-text/) have the rest.
