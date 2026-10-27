<script>
	import Status from '#lib/components/Status.svelte';
</script>

An **entry** is one piece of content in a collection: one article, for example. An entry doesn't hold its content directly. It points at its latest **revision**, and every save adds a new revision instead of changing the old one.

## Revisions are insert-only

A revision is written once and never updated or deleted. It holds:

- the content, as one canonical JSON document, stored as text exactly as `JSON.stringify` would write it;
- its SHA-256, written `sha256:<hex>`, which is what reviews and comparisons are bound to;
- the version of the content type it was validated against;
- the revision it was built on, who saved it, and when.

This isn't only a convention in the code. The database role the server runs as is granted `SELECT` and `INSERT` on revisions and nothing else, so even a bug in the server can't change or delete one.

The entry's **draft** is a pointer to its newest revision. Saving moves the pointer forward; nothing earlier changes. So the history of an entry is complete by construction, whichever way it was written: the Studio, the command line or a script, because they all use the same API.

## Why text, not a JSON column

PostgreSQL's `jsonb` type reorders keys and normalises numbers. That would break two promises: that a document loaded into the editor and saved again is byte for byte identical, and that a content hash computed in the browser matches the one the server stored. Revisions are stored as the canonical text, and the server checks every document against its content type before it is written. Data is never dropped silently: unknown fields are refused, not ignored.

## What reads see

- `GET …/entries/{entry}` returns the draft: the newest revision, with an `ETag` header naming that revision's id.
- `GET …/entries/{entry}/revisions` returns the history, newest first, and `…/revisions/{revision}` one revision's content.
- Readers on the website never see drafts at all: public delivery reads a separate copy of published revisions, as a database role that can't read drafts ([Releases and generations](/concepts/releases-and-generations/)).

## How it is tested

- **F01**: the real server is killed just before a save commits. No success was acknowledged and nothing was stored; a retry with the same key works.
- **F02**: killed just after the commit, before the answer. The revision is stored, and the retry returns the same result instead of storing it again.
- The [simulator](/concepts/simulator/) checks after every step of thousands of seeded runs that no acknowledged save is ever lost.

## Not built yet

Locales: one entry with a revision line per language. The data model was designed for it; the tables don't have it yet. <Status planned="later" /> Restoring an old revision as a new draft. <Status planned="P4" /> Archiving entries. <Status planned="not scheduled" />

## Design

[Module 01, domain](/design/modules/01-domain/) and [module 02, db](/design/modules/02-db/); decisions D-034, D-035 and D-040 in the [decisions](/design/decisions/). On GitHub: [docs/modules/01-domain.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/01-domain.md), [docs/modules/02-db.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/02-db.md).
