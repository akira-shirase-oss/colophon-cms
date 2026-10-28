<script>
	import Status from '#lib/components/Status.svelte';
</script>

A content type says what an entry of one collection may hold: its fields, their types and limits, and which field is the title. Definitions are versioned, parsed rather than trusted, and stored in one canonical form. The four routes and the six original field types came with step 1.5 (D-040). Step 3.6 added the `slug` field type and lets a change that narrows a definition through when no stored entry is in the way, naming the entries when some are (D-054, D-055, migration 0016); the Studio's content-type builder (Settings, then the type) makes every change through these routes. Milestone M, built on a branch and not merged, adds eleven more field types, references, objects, lists and page-builder blocks, single types, patterns and unique fields: they are listed here briefly, and in full in [Content modelling](/reference/content-modelling/). Handler: `crates/colophon-api/src/content_types.rs`; rules: `crates/colophon-domain/src/schema.rs`. The ideas are in [Content types and URL names](/concepts/content-types/).

## The definition

```json
{
  "name": "Pages",
  "title_field": "title",
  "fields": [
    { "id": "title", "label": "Title", "type": "text", "required": true, "max_chars": 120 },
    { "id": "slug", "label": "URL name", "type": "slug", "required": true, "source": "title" },
    { "id": "summary", "label": "Summary", "type": "text", "max_chars": 500, "multiline": true },
    { "id": "body", "label": "Body", "type": "rich_text", "required": true },
    { "id": "section", "label": "Section", "type": "enum", "values": ["guides", "news", "about"] },
    { "id": "reading_minutes", "label": "Reading time", "type": "number", "integer": true, "min": 1, "max": 120 },
    { "id": "featured", "label": "Featured", "type": "boolean" },
    { "id": "event_at", "label": "Event time", "type": "datetime" }
  ]
}
```

| Key | Rules | Refused with |
|---|---|---|
| (the whole) | A JSON object with `name`, `title_field`, `fields` and, optionally, `single`; any other key is refused, never dropped. | `not_an_object`, `unknown_field` |
| `name` | 1 to 100 characters, no control characters, no spaces at either end. | `invalid_name` |
| `fields` | 1 to 100 fields, ids unique. | `missing_field`, `invalid_fields`, `duplicate_field` |
| `title_field` | The id of a **required, single-line text** field. Lists show it. | `missing_field`, `invalid_title_field` |
| `single` | `true` for a [single type](/reference/content-modelling/#single-types), which holds at most one entry; default `false`, and written into the stored definition only when `true`. <Status building="M.4" /> | `wrong_type` |

Every field has:

| Key | Rules | Refused with |
|---|---|---|
| `id` | The key in content: 1 to 64 lowercase letters, digits and underscores, starting with a letter. | `invalid_id` |
| `label` | What editors see: 1 to 100 characters, no control characters, no spaces at either end. | `invalid_label` |
| `type` | One of the types below. | `invalid_type` |
| `required` | `true` or `false`, default `false`. Content must have a required field. | `wrong_type` |
| `description` | Help for editors, up to 500 characters; written only when set. <Status building="M.1" /> | `invalid_description` |
| `unique` | `true` for a [unique field](/reference/content-modelling/#unique-fields): a top-level single-line text, email, URL, number, date, time or date-time field only; written only when `true`. <Status building="M.4" /> | `invalid_unique`, `wrong_type` |

A field object holds only the common keys and its type's options; anything else is `unknown_field`. Refusals point into the request: `/definition/fields/2/max_chars` when creating, `/fields/2/max_chars` when changing.

## Field types

| `type` | Options | A value is | Value refused with |
|---|---|---|---|
| `text` | `max_chars` (1 to 100,000, default 200); `multiline` (default `false`) | A string of at most `max_chars` characters, not empty. Single-line text has no control characters and no spaces at either end; multi-line text may also hold line breaks and tabs. Refused, never trimmed. | `wrong_type`, `too_long`, `invalid_text` |
| `rich_text` | none | A [`colophon/rich-text` v1 document](/reference/rich-text/), stored in canonical form. | the [rich-text codes](/reference/rich-text/#error-codes), pointing inside the field |
| `number` | `integer` (default `false`); `min`, `max` (optional) | A whole number within ±9,007,199,254,740,991 when `integer`; otherwise below 1,000,000,000 in size with at most 6 decimal places. Within `min` and `max`. | `wrong_type`, `not_an_integer`, `too_precise`, `out_of_range` |
| `boolean` | none | `true` or `false`. | `wrong_type` |
| `enum` | `values`: 1 to 100 distinct options, each 1 to 100 characters, no control characters, no spaces at either end | One of `values`. | `not_an_option` |
| `datetime` | none | A UTC time written `YYYY-MM-DDTHH:MM:SSZ`, a real calendar date. | `invalid_datetime` |
| `slug` | `source` (optional) | An entry's URL name: 1 to 120 characters, lowercase ASCII letters and digits in groups joined by single hyphens, like `my-first-post`. | `invalid_slug` |

Milestone M adds `min_chars` and `pattern` to `text`, and the types `email`, `url`, `date`, `time`, `json`, `markdown`, `color`, `reference`, `object`, `list` and `blocks`. [Content modelling](/reference/content-modelling/#field-types) has each one's options, values, stored form and refusals. <Status building="M.1–M.4" />

Option errors: `max_chars` out of range is `out_of_range`; `min` or `max` that isn't a number is `wrong_type`, and one that isn't a value the field could hold, or a `max` below `min`, is `out_of_range`; `values` missing is `missing_field`, and empty, too many, duplicated or badly written options are `invalid_values`.

Content is checked against its type on every save: unknown keys are `unknown_field`, a missing required field `missing_field`, a value that isn't an object `not_an_object`. An optional field is left out rather than sent empty. The content is stored in canonical form: fields in definition order, absent optional fields left out.

## Slug fields

A `slug` field holds an entry's URL name (the editors' word; [URL names](/editors/url-names/)).

- A content type has at most one; a second is `duplicate_slug`, pointing at its `type`.
- `source`, if given, names one of the type's single-line text fields: the Studio suggests the URL name from it as you type. It is only a hint; the server never derives, lowercases or fixes a slug, it refuses one that breaks the rule (`invalid_source` for a bad `source`).
- **Unique per collection**, among drafts and among what is published, enforced by two unique indexes in PostgreSQL. Saving a slug another entry of the collection has, as its draft or published, is `409` `slug_taken`, naming that entry. A publish that would put live a slug another entry still publishes is refused whole, with the same reason ([Errors](/reference/api/errors/#conflicts)).
- Published entries carry their `slug` on `/content/v1` ([Delivery API](/reference/delivery-api/)). Reading an entry by its slug isn't built yet.

## Versions and ETags

Each change stores a new version; nothing is edited in place. A version as the API shows it:

```json
{
  "slug": "pages",
  "version": 3,
  "definition": { "name": "Pages", "title_field": "title", "fields": [ { "id": "title", "label": "Title", "type": "text", "required": true, "max_chars": 120, "multiline": false } ] },
  "definition_hash": "sha256:5d0e71a3c2b94f6e8a1d7c3b2e9f0a4b6c8d1e2f3a4b5c6d7e8f9a0b1c2d3e4f",
  "created_at": "2026-10-27T10:20:31.004812Z"
}
```

- `definition` is the canonical form, every default written out (`"required": false`, `"max_chars": 200`, `"multiline": false`, `"integer": false`), exactly the bytes stored and hashed. It matches what JavaScript's `JSON.stringify` writes for the same value, so the Studio can reproduce it.
- `definition_hash` is the SHA-256 of those bytes.
- The `ETag` is `"v<version>"`, and changing the type needs it in `If-Match`.
- Every saved revision records the version it was validated against (`schema_version` in a [publish request's items](/reference/api/publish-requests/#post-pchangesets)). Stored content is never rewritten.

## Changing a content type

A new definition replaces the current one only if every stored entry stays valid. Each change falls in one of three groups (`ContentType::successor`):

| Change | Outcome |
|---|---|
| A new name or labels; a new optional field; wider limits (`max_chars` up, single-line to multi-line, whole number to decimal, `min` or `max` relaxed or removed); more enum options; another `source` for the slug | always allowed |
| Removing a field, making one required, adding a required field, narrowing a limit, removing enum options, changing a field's type | allowed if **every current draft and every published revision** fits the new definition; otherwise `422` `needs_migration`, naming the entries in the way |
| Changing `title_field`; turning a field into or out of a slug field | always `422` `needs_migration`: the stored titles and slugs would disagree with content |

Before step 3.6 (D-040 alone), every change in the second group was refused whatever was stored. Milestone M's types follow the same three groups; [Content modelling](/reference/content-modelling/#changing-a-type) has the full list, including pointing a reference at another collection (always refused), and the two changes the database checks: making a field unique (refused while two drafts or two published entries share a value, each one in the way with the problem `value_taken`) and making a type single (refused with `too_many_entries` while it has more than one entry). <Status building="M.1–M.4" />

When a change needs checking, the server takes an exclusive lock on the collection before reading anything; entry saves, creates and publishes take the same lock shared, so no write can slip in between the check and the commit. It then validates every draft and every open publication of the collection, 200 at a time, off the request threads. More than 10,000 of them is refused without looking, with `too_many_to_check`. A migration tool for those cases doesn't exist yet.

The refusal names the first 20 drafts or publications in the way, what is wrong with each, and how many there are in all:

```json
{
  "error": {
    "code": "validation_failed",
    "message": "removing `subtitle` would make stored content invalid: 2 entries are in the way; change them first, or keep the old definition",
    "details": {
      "reason": "needs_migration",
      "pointer": "/fields",
      "entries": [
        {
          "entry": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
          "title": "Spring launch",
          "state": "draft",
          "problem": { "reason": "unknown_field", "pointer": "/subtitle", "message": "`subtitle` is not allowed here" }
        },
        {
          "entry": "0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607",
          "title": "Spring launch",
          "state": "published",
          "problem": { "reason": "unknown_field", "pointer": "/subtitle", "message": "`subtitle` is not allowed here" }
        }
      ],
      "total": 2
    },
    "request_id": "0193f6e2-8b9c-7d0e-9f1a-2b3c4d5e6f70"
  }
}
```

| Field | Meaning |
|---|---|
| `pointer` | Where in your new definition the change is. |
| `entries[].entry`, `.title` | The entry, and its title in the revision in the way. |
| `entries[].state` | `draft` (its current draft) or `published` (what readers get now). One entry can appear twice. |
| `entries[].problem` | The refusal that revision's content would get: `reason`, `pointer` into the content, `message`. |
| `total` | How many drafts and publications are in the way in all. |

A refusal of the third group has `reason` `needs_migration` and a `pointer`, with no `entries` or `total`.

## Routes

### `GET {P}/content-types`

The current version of each content type in the environment.

- **Who:** any member.

**Response:** `200`, by slug, all on one page (`next_cursor` always `null`). No `ETag`: read one type for that.

```json
{ "data": [ { "slug": "articles", "version": 1, "definition": { "name": "Articles", "title_field": "title", "fields": [ { "id": "title", "label": "Title", "type": "text", "required": true, "max_chars": 200, "multiline": false }, { "id": "body", "label": "Body", "type": "rich_text", "required": true } ] }, "definition_hash": "sha256:…", "created_at": "2026-10-20T08:00:12.004417Z" } ], "next_cursor": null }
```

Every project starts with `articles`, as above.

### `POST {P}/content-types`

Creates a content type, at version 1.

- **Who:** `schema.change` (owners, developers), on `*` or on the new slug.
- **Headers:** `Idempotency-Key` (required).
- **Body:**

```json
{ "slug": "pages", "definition": { "name": "Pages", "title_field": "title", "fields": [ { "id": "title", "label": "Title", "type": "text", "required": true } ] } }
```

| Field | Type | Rules |
|---|---|---|
| `slug` | string | Required. The collection's name in every path: 1 to 63 lowercase letters, digits and hyphens, not starting or ending with a hyphen. Not `members`, `settings`, `changesets` or `releases`, which name Studio pages. Unique in the environment. |
| `definition` | object | Required. [The definition](#the-definition). |

**Response:** `201 Created`, `ETag: "v1"`, `{"data": …}` with the version as shown above.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | `missing_permission` | Your role can't change content types here. |
| `409` | `conflict` | `content_type_exists` | A content type with this slug exists. |
| `409` | `conflict` | `idempotency_key_reused` | The key was used for a different request. |
| `422` | `validation_failed` | `invalid_slug` | `/slug` breaks the rule. |
| `422` | `validation_failed` | `reserved_slug` | `/slug` names a Studio page. |
| `422` | `validation_failed` | a definition code | The definition breaks a rule; the pointer starts with `/definition`. |
| `428` | `precondition_required` | | No `Idempotency-Key`. |

### `GET {P}/content-types/{slug}`

The current version of one content type.

- **Who:** any member.

**Response:** `200`, `ETag: "v<version>"`, `{"data": …}` with the version.

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No content type with this slug here. |

### `PUT {P}/content-types/{slug}`

Stores a new definition as the next version, if [the change is allowed](#changing-a-content-type).

- **Who:** `schema.change` (owners, developers), on `*` or on this collection.
- **Headers:** `If-Match: "v<version>"`, the version you edited, and `Idempotency-Key` (both required).
- **Body:** the new definition itself, not wrapped:

```json
{
  "name": "Pages",
  "title_field": "title",
  "fields": [
    { "id": "title", "label": "Title", "type": "text", "required": true, "max_chars": 160 },
    { "id": "slug", "label": "URL name", "type": "slug", "source": "title" }
  ]
}
```

**Response:** `200`, `ETag: "v<new version>"`, `{"data": …}` with the new version.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` isn't one content type `ETag`, such as `"v3"`, or the key is malformed. |
| `403` | `forbidden` | `missing_permission` | Your role can't change this content type. |
| `404` | `not_found` | | No content type with this slug. |
| `409` | `conflict` | `stale_version` | It changed since your version; the `ETag` header names the current one. Nothing was written. |
| `409` | `conflict` | `idempotency_key_reused` | The key was used for a different request. |
| `422` | `validation_failed` | a definition code | The new definition breaks a rule. |
| `422` | `validation_failed` | `needs_migration` | The change would make stored content invalid, or needs a migration whatever is stored. With `entries` and `total` when entries are in the way. |
| `422` | `validation_failed` | `too_many_to_check` | More than 10,000 drafts and publications to check. |
| `422` | `validation_failed` | `too_many_entries` | `single` turned on while the collection has more than one entry; the pointer is `/single`. <Status building="M.4" /> |
| `428` | `precondition_required` | | No `If-Match`, or no `Idempotency-Key`. |

## Proof

`crates/colophon-server/tests/content_types.rs` against the real binary: `a_content_type_is_defined_and_its_entries_follow_it`, `a_content_type_changes_only_in_ways_that_keep_its_entries_valid`, `a_narrowing_change_goes_through_when_nothing_stored_is_in_the_way` and `a_slug_belongs_to_one_entry_and_reaches_the_public_api` (3.6), and `only_owners_and_developers_change_content_types_and_strangers_see_nothing`; `crates/colophon-db/tests/slugs.rs` for the unique indexes; the unit tests in `schema.rs`; the load scenario `perf/k6/content-types.js`. Milestone M adds `references.rs`, `singles.rs`, `unique.rs` and `a_pattern_is_checked_against_stored_content_and_holds_new_content_to_it`, and the load scenario `perf/k6/modelling.js` ([Content modelling](/reference/content-modelling/#how-it-is-tested)).

## Not built yet

- Migrations, with a plan, a dry run, approval and per-entry outcomes, for the changes refused today. <Status planned="M" />
- Defaults, conditional fields and shared components. <Status planned="M" />
- URL names per locale. <Status planned="E" />

See the [roadmap](/resources/roadmap/).
