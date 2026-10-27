<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **content type** says what an entry of a collection holds: its fields, their kinds and their limits. Every save is checked against it on the server, whatever sent it, and a content type can only change in ways that keep every stored entry valid.

## A definition

A definition has a name, a title field and 1 to 100 fields. Each field has an id, a label, `required`, and one of six types:

| Type | Options | Stored as |
|---|---|---|
| `text` | `max_chars`, `multiline` | a non-empty string with no surrounding whitespace or control characters, refused rather than trimmed |
| `rich_text` | none | a [colophon/rich-text v1](/reference/rich-text/) document |
| `number` | `integer`, `min`, `max` | safe integers, or decimals with at most six places below 10^9, so Rust and the browser write the same bytes |
| `boolean` | none | `true` or `false` |
| `enum` | `values` | one of the values |
| `datetime` | none | `YYYY-MM-DDTHH:MM:SSZ` |

An empty value is left out, not written as an empty string, so "empty" has one spelling. Every environment starts with `articles`: a required single-line `title` of up to 200 characters and a required rich-text `body`.

```json
{
  "name": "Posts",
  "title_field": "headline",
  "fields": [
    { "id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120 },
    { "id": "body", "label": "Body", "type": "rich_text", "required": true },
    { "id": "featured", "label": "Featured", "type": "boolean" },
    { "id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"] }
  ]
}
```

## Safe changes only

Definitions are versioned and insert-only: each new version names the one before, and every revision records the version it was validated against. A new version is accepted when it can't make a stored entry invalid:

- a new optional field;
- a wider limit (a longer `max_chars`, a lower `min`, a higher `max`);
- more enum values;
- new labels or a new name.

Anything else (removing a field, making one required, narrowing a limit, removing a value, changing a type) is refused with `needs_migration`. Updating a definition takes `If-Match` with its `ETag` (`"v3"`, for example), so two people changing it at once can't overwrite each other (`409 stale_version`). Only owners and developers change content types.

A few names are reserved because Studio pages sit beside collections: `members`, `settings`, `changesets` and `releases`.

## Being built now

On the step 3.6 branch, not yet in this code:

- **Changes checked against stored content** (D-055). A narrowing change is allowed when no draft or published revision would break, and refused with the first entries in the way otherwise. <Status building="step 3.6" />
- **A URL name field** (`slug`, D-054), unique per collection among drafts and among what is published, enforced by the database. <Status building="step 3.6" />
- **A content-type builder in the Studio**, so definitions no longer need to be written as JSON through the API. <Status building="step 3.6" />

Later: image fields with media <Status planned="P4" />, references between entries, repeatable groups of fields, and migrations with a reviewed plan for the changes that safe changes can't cover <Status planned="later" />.

## How it is tested

The domain's unit tests cover every type's limits and the canonical form; `crates/colophon-server/tests/content_types.rs` runs definitions and entries end to end against the real server; `perf/k6/content-types.js` is its load scenario.

## Design

[Module 01, domain](/design/modules/01-domain/); decision D-040 in the [decisions](/design/decisions/). On GitHub: [docs/modules/01-domain.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/01-domain.md).
