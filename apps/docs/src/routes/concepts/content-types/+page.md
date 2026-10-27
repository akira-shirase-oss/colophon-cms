<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **content type** says what an entry of a collection holds: its fields, their kinds and their limits. Every save is checked against it on the server, whatever sent it, and a content type can only change in ways that keep every stored entry valid. Versioned definitions with six field types are done (step 1.5). Step 3.6, on the branch `p3/3.6-modelling` and not merged, adds a seventh type for **URL names** (slugs), checks a change against the content actually stored instead of refusing every narrowing, and gives the Studio a builder. References, components, lists and the rest of milestone M are planned.

## A definition <Status done="1.5" />

A definition has a name, a title field and 1 to 100 fields. Each field has an id (`^[a-z][a-z0-9_]{0,63}$`, the key in content), a label, `required`, and one of these types:

| Type | Options | Stored as |
|---|---|---|
| `text` | `max_chars` (default 200, at most 100,000), `multiline` | a non-empty string with no surrounding whitespace or control characters, refused rather than trimmed |
| `rich_text` | none | a [colophon/rich-text v1](/reference/rich-text/) document |
| `number` | `integer`, `min`, `max` | safe integers, or decimals with at most six places below 10^9, so Rust and the browser write the same bytes |
| `boolean` | none | `true` or `false` |
| `enum` | `values` (1 to 100) | one of the values |
| `datetime` | none | `YYYY-MM-DDTHH:MM:SSZ` |
| `slug` <Status building="3.6" /> | `source` | a URL name: see below |

An empty value is left out, not written as an empty string, so "empty" has one spelling. Definitions and content are written canonically, as `JSON.stringify` would, and each version's SHA-256 identifies it. Every environment starts with `articles`: a required single-line `title` of up to 200 characters and a required rich-text `body`.

```json
{
  "name": "Posts",
  "title_field": "headline",
  "fields": [
    { "id": "headline", "label": "Headline", "type": "text", "required": true, "max_chars": 120 },
    { "id": "url", "label": "URL name", "type": "slug", "source": "headline" },
    { "id": "body", "label": "Body", "type": "rich_text", "required": true },
    { "id": "featured", "label": "Featured", "type": "boolean" },
    { "id": "section", "label": "Section", "type": "enum", "values": ["News", "Guides"] }
  ]
}
```

A collection's own name in URLs (`posts`) is 1 to 63 lowercase letters, digits and hyphens. Four names are reserved because Studio pages sit beside collections: `members`, `settings`, `changesets` and `releases` (`reserved_slug`).

## URL names <Status building="3.6" />

A `slug` field holds the part of a web address that names an entry, such as `my-first-post` (decision D-054). On the step 3.6 branch:

- A content type has at most one (`duplicate_slug` for a second). Its optional `source` names a single-line text field the Studio suggests the URL name from (`invalid_source` otherwise); it is only a hint, and the server never derives a URL name.
- A URL name is 1 to 120 lowercase ASCII letters and digits, in groups joined by single hyphens. Anything else is refused with `invalid_slug` and the field's pointer, never normalised by the server: the Studio normalises as you type, so what is stored is what was sent, like every other text rule.
- **No two entries of a collection share one**, among drafts and among what is published. Each revision stores its URL name; the entry copies its draft's (`entries.draft_slug`) and a publication its revision's (`publications.slug`). Migration 0016 adds two partial unique indexes: `entries_draft_slug` over drafts, and `publications_live_slug` over open publications. The database refuses a second one itself, because a check in the application alone could race (spec §07).
- A save whose URL name another entry has, as its draft or published, gets `409` with `details.reason` `slug_taken` and the entry in the way in `details.entries` (its id, title, and `draft` or `published`). That lookup only words the refusal; a save that loses a race to the index gets the same `409` without a name.
- A release that would publish a URL name another entry still publishes is **refused whole**, `409 slug_taken`, and nothing commits. Publish that entry's new URL name first, or in the same publish request ([Releases and generations](/concepts/releases-and-generations/)).
- Public delivery answers carry `slug` when an entry has one, and the reference site moves its article pages from `/articles/{entry id}/` to `/articles/{slug}/` (an article without one keeps its id), before the site's addresses go public: a public URL is hard to change later.

Changing a field into or out of a slug is refused with `needs_migration`: stored URL names would disagree with the content. Locales join the uniqueness rule when they exist. <Status planned="E" /> A changed URL name doesn't redirect from the old one yet. <Status planned="P4" />

The editor's side, in plain words, is [URL names](/editors/url-names/).

## Safe changes <Status done="1.5" />

Definitions are versioned and insert-only: each new version names the one before, and every revision records the version it was validated against. These changes can never make stored content invalid, so they are accepted without looking at it:

- a new optional field;
- a wider limit: a longer `max_chars`, a lower `min` or higher `max` (or none), a single-line text becoming multi-line, an integer field becoming a decimal one;
- more enum values;
- new labels or a new name.

Updating a definition takes `If-Match` with its `ETag` (`"v3"`, for example) and an `Idempotency-Key`, so two people changing it at once can't overwrite each other (`409 stale_version`). Creating or changing a content type needs `schema.change` for that collection: owners and developers have it by default.

On `main`, anything else (removing a field, making one required, narrowing a limit, removing a value, changing a type) is refused with `needs_migration`, whatever is stored.

## Changes checked against stored content <Status building="3.6" />

Decision D-055 amends that rule on the step 3.6 branch: a narrowing change is **allowed when nothing stored is in the way**. `colophon_domain::schema::ContentType::successor` sorts a change into one of three answers:

| Answer | Changes | What happens |
|---|---|---|
| `Compatible` | the safe changes above | accepted |
| `CheckContent` | removing a field, making one required, a new required field, narrowing a limit, removing enum values, changing a field's type | accepted only if the new definition accepts every entry's draft and every published revision |
| `Refused` | a different title field; a field turned into or out of a slug | `needs_migration`, whatever is stored |

When stored content is in the way, the answer is `422` with `details.reason` `needs_migration`, the first 20 entries in the way in `details.entries` (each with its title, whether its draft or its published version is in the way, and the refusal its content would get: code, pointer and message) and the total in `details.total`. Change those entries, or keep the old definition.

Only drafts and open publications are checked. Publishing refuses a stale publish request, so only a current draft can become published, and older revisions stay valid under the version they recorded; they are never rewritten. Rollback, when it comes, will check old revisions against the current version before republishing them. <Status planned="P4" />

### Why no write slips in between

Every entry write and every publish takes a shared, transaction-scoped advisory lock on its collection before it reads the content type or its items. A change takes the same lock exclusively before it reads anything. So the check sees every write that committed before it, and every write that starts after it waits and then validates against the new version. The cost is one more statement on every entry write; a collision of the 32-bit lock key only makes two collections wait for each other's changes.

### Bounds

The check validates documents in pages of 200 on the blocking thread pool, off the request workers. A collection with more than 10,000 drafts and publications to check is refused with `too_many_to_check` (`422`) without looking: that change needs a migration, which doesn't exist yet.

## The builder in the Studio <Status building="3.6" />

Settings, then Content types, opens a builder (`TypeBuilder.svelte`): rename the type and its fields, add fields of every type, change limits and whether a field is required, and remove fields. The server decides what is safe; a refusal lists the entries in the way with links to them. In the builder, field ids, field types and the title field are fixed once saved. Slug fields get a *From title* suggestion in the editor that follows the server's rule.

## Not built yet

- References (one and many), components and nested fields, bounded lists, block lists, single types, and email, URL, date, time, JSON, Markdown and colour fields; validation by pattern, uniqueness, length and value; defaults, descriptions, groups and tabs. <Status planned="M" />
- Migrations with a plan, a dry run, approval and per-entry outcomes, for the changes D-055 still refuses. <Status planned="M" />
- Image fields, with the media library. <Status planned="P4" />
- Redirects when a URL name changes. <Status planned="P4" />

## How it is tested

- The domain's unit tests cover every type's limits, the canonical form, URL names and the three kinds of change.
- `crates/colophon-server/tests/content_types.rs` runs definitions and entries end to end against the real server: a change refused with the entries in its way and accepted once they are fixed, a narrowing change accepted when nothing stored is in the way, a URL name that belongs to one entry and reaches the public API, and only owners and developers changing types.
- `crates/colophon-db/tests/slugs.rs` runs against PostgreSQL: two drafts never share a URL name, a release never publishes one another entry still publishes, a change waits for the writes under way, and the check sees every draft and publication. <Status building="3.6" />
- `apps/studio/tests/server/types.spec.ts` drives the builder in a real browser; `apps/testbed/tests/content-types.spec.ts` covers the API; `perf/k6/content-types.js` is the load scenario.

## Design

[Module 01, domain](/design/modules/01-domain/) and decision D-040 in the [decisions](/design/decisions/) describe what is on `main`. Step 3.6's design is on its branch: D-054 and D-055 in [DECISIONS.md](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/DECISIONS.md), [module 01](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/modules/01-domain.md), [module 02](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/modules/02-db.md) and [COMPARISON.md](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/COMPARISON.md), which compares content modelling with Strapi's and Sanity's. The API's side is in [Content types](/reference/api/content-types/).
