<script>
	import Status from '#lib/components/Status.svelte';
</script>

An **entry** is one piece of content in a collection: one article, for example. An entry doesn't hold its content directly. It points at its latest **revision** (a *version* in the Studio's plain words), and every save adds a new revision instead of changing the old one.

## Revisions are insert-only

A revision is written once and never updated or deleted. It holds:

- the content, as one canonical JSON document, stored as text exactly as `JSON.stringify` would write it (at most 263,168 bytes);
- its SHA-256, written `sha256:<hex>`, which reviews and comparisons are bound to;
- its collection and the **version of the content type** it was validated against (`schema_version`), so it stays valid under the definition it was written for, whatever changes later;
- a copy of its title, for lists, so a list never parses every document;
- the revision it was built on (`base_revision_id`), who saved it, and when.

This isn't only a convention in the code. Migration 0002 grants the role the server runs as (`colophon_api`) `SELECT` and `INSERT` on `revisions` and nothing else, so even a bug in the server can't change or delete one. A revision's base must be one of the same entry's revisions: a composite foreign key checks it.

## The draft pointer

The entry's **draft** is a pointer to its newest revision, `entries.draft_revision_id`, a deferred composite foreign key that can only name one of the entry's own revisions. Saving moves the pointer forward with a conditional update that only succeeds if it still names the revision the save was built on ([Saves and conflicts](/concepts/saves-and-conflicts/)). Nothing earlier changes, so the history of an entry is complete by construction, whichever way it was written: the Studio, the command line or a script, because they all use the same API.

The two statements that move the pointer (creating an entry and saving one) also copy two things from the new draft onto the entry:

- **its title**, `entries.draft_title` (migration 0015, from the title-search fix `065e614`). Step 3.5's title search found each entry's draft revision first and filtered after, so one search of a 512-entry collection made 512 lookups and took 157 ms in CI. Search now filters on the entry's own copy, served by a trigram index, before it looks anything up. `crates/colophon-db/tests/entries.rs` checks the copy against the draft and that search follows a renamed draft. <Status done="fix 065e614" />
- **its URL name**, `entries.draft_slug` (migration 0016), when its content type has a slug field. A unique index on it is what stops two drafts of a collection sharing a URL name ([Content types and URL names](/concepts/content-types/)). <Status done="3.6" />

Neither copy is a foreign key, which would add a unique index on `revisions` that every save writes; tests hold each copy equal to the draft revision's value instead.

## Why text, not a JSON column

PostgreSQL's `jsonb` type reorders keys and normalises numbers. That would break two promises: that a document loaded into the editor and saved again is byte for byte identical, and that a content hash computed in the browser matches the one the server stored. Revisions are stored as the canonical text, and the server checks every document against its content type before it is written. Data is never dropped silently: unknown fields are refused, not ignored.

## What reads see

- `GET …/entries/{entry}` returns the draft: the newest revision, with an `ETag` header naming that revision's id.
- `GET …/entries/{entry}/revisions` returns the history, newest first, and `…/revisions/{revision}` one revision's content. The Studio's history and comparison views read these ([Versions and comparing](/editors/versions/)).
- `GET …/entries?q=` searches titles through the entry's copy of its draft title.
- Readers on the website never see drafts at all: public delivery reads a separate copy of published revisions, as a database role that can't read drafts ([Releases and generations](/concepts/releases-and-generations/)).

Reading needs `content.read` for the entry's collection; creating needs `content.create` and saving `content.update` ([Roles and permissions](/concepts/roles-and-permissions/)).

## How it is tested

- **F01**: the real server is killed immediately before a save commits. No success was acknowledged and nothing was stored; a retry with the same key works.
- **F02**: killed immediately after the commit, before the answer. The revision is stored, and the retry returns the same result instead of storing it again.
- The [simulator](/concepts/simulator/) checks after every step of thousands of seeded runs that no acknowledged save is ever lost.
- `crates/colophon-db/tests/slugs.rs` checks that the draft's URL name copy always matches the draft revision. <Status done="3.6" />

## Not built yet

- Locales: one entry with a line of drafts and publications per language. The data model was designed for it; the tables don't have it yet. <Status planned="E" />
- Restoring an old revision as a new draft. <Status planned="P4" />
- Deleting or archiving entries. No route deletes an entry; the `content.delete` permission exists so roles can name it when one does. <Status planned="not scheduled" />

## Design

[Module 01, domain](/design/modules/01-domain/) and [module 02, db](/design/modules/02-db/); decisions D-034, D-035 and D-040 in the [decisions](/design/decisions/). On GitHub: [docs/modules/01-domain.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/01-domain.md), [docs/modules/02-db.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/02-db.md).
