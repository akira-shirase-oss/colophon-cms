<script>
	import Status from '#lib/components/Status.svelte';
</script>

Everything a content type can say, in one place: every field type with its options and the form its values are stored in, references between entries, objects, lists and page-builder blocks, single types, patterns and unique fields, and what a change to a type may do to stored content. The server decides all of it, in Rust (`crates/colophon-domain/src/schema.rs`, checked in the database where a rule spans entries); a browser check could only ever be a hint. The ideas are in [Content types and URL names](/concepts/content-types/), the routes in [Content types](/reference/api/content-types/) and [Entries](/reference/api/entries/), and every refusal code in [Errors](/reference/api/errors/#content-and-content-types).

## Where this stands

Milestone M (content modelling parity) is being built on a branch and isn't merged. Each step below has its tests, a page in the testbed (`/modelling`) and a load scenario (`perf/k6/modelling.js`); the scenario's million-request runs haven't been made yet.

- The seven types of steps 1.5 and 3.6: text, rich text, number, yes/no, choice, date-time and URL name. <Status done="3.6" />
- Seven more field types, a `description` on any field and a `min_chars` on text (D-064). <Status building="M.1" />
- References between entries that never dangle (D-065, migration 0024). <Status building="M.2" />
- Objects, lists and page-builder blocks (D-066, migration 0025). <Status building="M.3" />
- Single types (D-067, migration 0026), text patterns (D-069) and unique fields (D-070, migration 0027). <Status building="M.4" />
- Defaults, conditional fields, shared components, and migrations with a plan and a dry run. <Status planned="M" />

The Studio's content-type builder still offers only the first seven types, so the newer types and options are defined through the API for now. When the builder saves, it sends back the whole definition it loaded, so options it doesn't show, such as a pattern, are kept.

## A definition

A definition is a JSON object with these keys, and no others:

| Key | Rules | Refused with |
|---|---|---|
| `name` | 1 to 100 characters, no control characters, no spaces at either end. | `invalid_name` |
| `title_field` | The id of a **required, single-line text** field at the top level. Lists and search show it. | `missing_field`, `invalid_title_field` |
| `single` | `true` makes it a [single type](#single-types): at most one entry. Default `false`. | `wrong_type` |
| `fields` | 1 to 100 fields, ids unique. | `missing_field`, `invalid_fields`, `duplicate_field` |

Every field has these keys, and its type's options:

| Key | Rules | Refused with |
|---|---|---|
| `id` | The key in content: 1 to 64 lowercase letters, digits and underscores, starting with a letter. | `invalid_id` |
| `label` | What editors see: 1 to 100 characters, no control characters, no spaces at either end. | `invalid_label` |
| `description` | Help for editors, up to 500 characters, no control characters, no spaces at either end. An empty string means none. | `invalid_description` |
| `type` | One of the [field types](#field-types). | `invalid_type` |
| `required` | `true` or `false`, default `false`. Content must have a required field; an optional one is left out rather than sent empty. | `wrong_type` |
| `unique` | `true` makes no two entries share a value: [unique fields](#unique-fields). Default `false`. | `invalid_unique`, `wrong_type` |

Anything else on a field, including an option that belongs to another type, is `unknown_field`, never ignored. Refusals point into the request: `/fields/2/max_chars` when changing a type, `/definition/fields/2/max_chars` when creating one.

## Field types

"Characters" means Unicode code points, as Rust counts them. Every value is refused rather than repaired, except where the table says it is written in a standard form.

| `type` | Options | A value is | Stored as | Refused with |
|---|---|---|---|---|
| `text` | `max_chars` (1 to 100,000, default 200); `min_chars` (1 to `max_chars`); `multiline` (default `false`); `pattern` ([patterns](#patterns)) | A string, not empty, within `min_chars` and `max_chars`. Single-line text has no control characters and no spaces at either end; multi-line text may also hold line breaks and tabs. | as sent | `wrong_type`, `too_long`, `too_short`, `invalid_text`, `pattern_mismatch` |
| `rich_text` | none | A [`colophon/rich-text` v1 document](/reference/rich-text/). | its canonical form | the [rich-text codes](/reference/rich-text/#error-codes) |
| `number` | `integer` (default `false`); `min`, `max` | A whole number within ±9,007,199,254,740,991 when `integer`; otherwise below 1,000,000,000 in size with at most 6 decimal places. Within `min` and `max`. | as `JSON.stringify` writes it: `1.50` is stored `1.5` | `wrong_type`, `not_an_integer`, `too_precise`, `out_of_range` |
| `boolean` | none | `true` or `false`. | as sent | `wrong_type` |
| `enum` | `values`: 1 to 100 distinct options of 1 to 100 characters | One of `values`. | as sent | `not_an_option` |
| `datetime` | none | A UTC time written `YYYY-MM-DDTHH:MM:SSZ`, on a real calendar date. | as sent | `invalid_datetime` |
| `slug` | `source` | An entry's URL name: 1 to 120 lowercase ASCII letters and digits, in groups joined by single hyphens. At most one per type, at the top level, unique in the collection ([URL names](/concepts/content-types/#url-names)). | as sent | `invalid_slug` |
| `email` | none | `local@domain`: a local part of at most 64 characters in the unquoted form of RFC 5322 (ASCII letters, digits, dots and the symbols that form allows; no dot at either end, no two dots together) and a host name with at least one dot; 254 characters at most. Checked for shape, never for delivery. | as sent | `invalid_email` |
| `url` | none | An `http` or `https` address with a host, no user name or password, no spaces, at most 2,048 characters. | its WHATWG form, what a browser's `new URL(text).href` gives: `HTTPS://Example.COM` is stored `https://example.com/` | `invalid_url` |
| `date` | none | A real calendar date, `YYYY-MM-DD`. | as sent | `invalid_date` |
| `time` | none | A time of day, `HH:MM` or `HH:MM:SS`, 24-hour, `00:00` to `23:59:59`. | as sent, so `09:30` and `09:30:00` are different values | `invalid_time` |
| `json` | none | Any JSON value up to 64 KiB, nested at most 32 levels; its numbers follow the `number` rules. | object keys sorted as JavaScript sorts them (by UTF-16 code units), numbers as `JSON.stringify` writes them | `invalid_json`, `too_long` |
| `markdown` | `max_chars` (1 to 100,000, default 100,000) | Markdown source: multi-line text, not empty, within `max_chars`. Colophon stores it; your site renders it. | as sent | `wrong_type`, `too_long`, `invalid_text` |
| `color` | none | A colour written `#rrggbb` in lowercase hexadecimal. `#1A2B3C` is refused, not lowercased. | as sent | `invalid_color` |
| `reference` | `to`, `many`, `max` | One entry's id, or a list of them: [references](#references). | as sent | `invalid_reference`, `duplicate_reference`, `too_many`, `wrong_type`; `reference_not_found` from the database |
| `object` | `fields` | An object holding its own fields: [structures](#objects-lists-and-blocks). | its fields in definition order | `not_an_object`, `unknown_field`, `missing_field` |
| `list` | `of`, `max` | A list of values of one kind. | in its order | `wrong_type`, `invalid_list`, `too_many` |
| `blocks` | `blocks`, `max` | A page builder: a list of blocks, each one of the kinds offered. | each block with `_block` first | `wrong_type`, `invalid_list`, `too_many`, `unknown_block` |

Option errors: a limit outside its range, a `min_chars` above `max_chars`, a `min` or `max` the field couldn't hold, or a `max` below `min` is `out_of_range`; an enum without `values` is `missing_field`, and empty, too many, duplicated or badly written options are `invalid_values`. Type ids use the spelling APIs use (`color`); the prose here says "colour".

## The stored form

Content and definitions are both written in one canonical form, the exact bytes stored and hashed with SHA-256, so the same thing always has the same hash and the Studio can reproduce it:

- **Content:** fields in definition order, optional fields that weren't sent left out, every value in the form the table above gives, nested objects and blocks the same way. Each version's `content_hash` is the hash of these bytes.
- **Definitions:** keys in a fixed order, `name`, `title_field`, then `fields`. Each field writes `id`, `label`, `type` and `required`, then its options with their defaults filled in (`"max_chars":200,"multiline":false` for plain text). The newer options (`single`, `description`, `unique`, `min_chars`, `pattern`) are written only when set, so a definition saved before they existed keeps its bytes and its hash, which publish requests pin.

The `code` field of the [example below](#a-worked-example), as stored:

```json
{"id":"code","label":"Post code","description":"Three capital letters, a hyphen and four digits, like NEW-0042.","type":"text","required":false,"unique":true,"max_chars":8,"multiline":false,"pattern":"[A-Z]{3}-[0-9]{4}"}
```

## References

A `reference` field names entries of one collection by their ids (D-065):

| Option | Rules |
|---|---|
| `to` | Required. The slug of the collection the entries belong to; it may be the type's own collection. |
| `many` | `true` for a list of entries; default `false`, one entry. |
| `max` | Only with `many`: 1 to 100 entries, default 100. Without `many` it is `out_of_range`. |

A value is an entry's id as a lowercase UUID (`"0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d"`), or with `many` a list of 1 to `max` distinct ids. An uppercase id, an empty list or one id twice is refused before anything is stored (`invalid_reference`, `duplicate_reference`).

- **They never dangle.** Every version's references are stored beside it, with a foreign key to the entry and its collection, so an id that names no entry of `to` can't be saved: `422` with `details.reason` `reference_not_found`, pointing at the field (`/author`), at the place in a list (`/related/1`), or deep inside a structure (`/sections/1/by`).
- **Drafts and readers are kept apart.** A draft's references are its own version's; what readers get references only what the published version names.
- **Publishing keeps readers whole.** A release that would publish an entry naming one that isn't published is refused whole, `409` `references_unpublished`; so is a release that would take down an entry a published entry still names, `409` `still_referenced`. Putting both entries in the same publish request works, either way round. Only the release's own entries are checked.
- **Referenced by.** Reading one entry (`GET` on its address) lists in `referenced_by` the entries whose draft or published version names it, up to 100: each with `entry_id`, `collection`, `field` (the path, such as `sections/1/by`), `in_draft` and `published`. Entries are left out from collections your role can't read, or can read only its own entries of.
- **Readers get ids.** `/content/v1` answers a reference with the ids as stored. Fetching the entries they name in the same request is part of the planned query API. <Status planned="D" />

Pointing a reference field at another collection, at any depth, always needs a migration (`needs_migration`): what is stored names entries of the old one. Turning one reference into a list, or a list into one, is checked against stored content.

## Objects, lists and blocks

Three types hold other values (D-066), Strapi's components and dynamic zones, Sanity's objects and arrays:

| `type` | Options | A value is |
|---|---|---|
| `object` | `fields`: 1 to 100 field definitions, like a type's own | An object holding those fields. Unknown keys are refused, required ones must be there. |
| `list` | `of`: what each item is, written `{"type": …}` with that type's options and nothing else; `max`: 1 to 100, default 100 | A list of 1 to `max` values. An item can't be a list or blocks: put those in an object. |
| `blocks` | `blocks`: 1 to 20 kinds of block, each `{"name", "label", "fields"}` with a distinct name written like a field id; `max`: 1 to 100, default 100 | A list of 1 to `max` blocks. Each is an object with `_block` naming its kind, and that kind's fields. |

- **At most three deep.** An object in a list in a block is as deep as it goes; a fourth level is `too_deep`.
- **Bounded.** No list or page builder holds more than 100 items, and an empty one is refused (`invalid_list`): leave the field out instead. More than `max` is `too_many`.
- **Some things stay at the top.** URL names, unique fields and the title field belong to the type itself, never inside a structure (`invalid_type`, `invalid_unique`). A list whose items are lists or blocks is `invalid_type` at `/fields/n/of/type`.
- **Refusals point where the value is:** `/faq/0/q` for a missing question in the first item, `/sections/1/_block` for a block of a kind that isn't offered.
- **Permissions see the top.** A role's [field permissions](/concepts/roles-and-permissions/) name top-level fields, so a structure is read or changed as a whole.
- A reference inside a structure is stored and checked like any other, with its path.

Each structure is defined inline in its type. Shared components, one definition used by many types, are planned for M.4. <Status planned="M.4" />

## Single types

A type with `"single": true` holds at most one entry in each environment, such as a site's settings or its home page (D-067):

- **The database says so.** Each entry copies the flag when it is created, and a partial unique index (`entries_single`) refuses a second, however many creations race: `409` with `details.reason` `single_exists`. Edit the one entry instead.
- **Making a type single** needs at most one entry in it; otherwise `422` `too_many_entries`, pointing at `/single`. Making it plural again only clears the flag.
- `single` is written into the stored definition only when it is `true`.

## Patterns

A text field's `pattern` is a regular expression the **whole** value must match (D-069): `[A-Z]{3}-[0-9]{4}` accepts `NEW-0042` and refuses `new-0042`, `xNEW-0042` and `NEW-00421`. You don't add `^` and `$`; the server wraps the pattern as `^(?:pattern)$`.

- **Rust `regex` syntax, matched in linear time.** The server uses the Rust `regex` crate, whose matching time grows only with the length of the value, so no pattern can stall a save the way a backtracking engine (JavaScript's, PCRE) can. The price is two features it doesn't have: look-around (`(?=…)`, `(?!…)`) and back-references (`\1`).
- **`\d`, `\w` and `\b` are Unicode-aware.** `\d` matches digits from every script, Arabic-Indic and Devanagari among them; write `[0-9]` for ASCII digits only. Matching is case-sensitive unless the pattern starts with `(?i)`, and `.` doesn't match a line break in multi-line text unless it starts with `(?s)`.
- **Checked where it is defined.** A pattern is 1 to 500 characters and compiles within 1 MiB. One that doesn't compile, uses look-around or a back-reference, or is too large is refused when the type is saved: `422` `invalid_pattern` at `/fields/n/pattern`.
- **Checked on every save.** A value that doesn't match is `422` `pattern_mismatch` at the value, inside objects, lists and blocks too (`/tags/1`).
- **Changing it.** Adding or changing a pattern is checked against every draft and published version (`needs_migration` with each one in the way, its problem `pattern_mismatch`); removing one is always allowed. Patterns compare by their text, so saving the same pattern again changes nothing.
- Only `text` takes a pattern; on `markdown` it is `unknown_field`.

## Unique fields

`"unique": true` makes a field's value belong to one entry (D-070): no two drafts share it, and no two published entries do. It works on a top-level single-line `text`, `email`, `url`, `number`, `date`, `time` or `datetime` field; anywhere else, including inside a structure, it is `422` `invalid_unique` at `/fields/n/unique`.

- **The database says so.** Each entry claims the values of its draft and of what it publishes in `unique_claims` (migration 0027), and a unique constraint refuses a second claim on a value however writes race. Eight creates of one value at once leave exactly one entry with it (`racing_writes_of_one_value_leave_it_to_exactly_one_entry`).
- **Values compare exactly as stored.** `Ada@example.com` and `ada@example.com` are different values, and so are `09:30` and `09:30:00`. Where a type stores a standard form, the form is what counts: `1.50` and `1.5`, or `HTTPS://Example.COM` and `https://example.com/`, are the same value. Case-insensitive comparison isn't offered.
- **A value left out claims nothing.** Any number of entries may leave an optional unique field empty.
- **Saving.** A create or save whose value another entry has, as its draft or as what it publishes, gets `409` `value_taken`, pointing at the field. An entry keeps its own value from save to save.
- **Drafts and what is published.** Once an entry is published with a value, its draft may move on to another; the old value stays claimed by what it publishes until a release replaces that. A release that would publish a value another entry still publishes is refused whole, `409` `value_taken`. Moving a value from one entry to another works when both are in the same publish request.
- **Who is named.** The refusal names the entry in the way in `details.entries` (its id, title, and `draft` or `published`) only when your role may read that entry's title; otherwise it says only that the value is taken. A write that lost a race to the database names no one. `slug_taken` follows the same rule.
- **Changing a type.** Making a field unique claims every stored value under the type's exclusive lock. While two drafts or two published entries share a value, the change is refused: `422` `needs_migration` at `/fields/n/unique`, each entry in the way with the problem `value_taken`, and `total`. More than 10,000 drafts and publications to check is `too_many_to_check`. Dropping `unique` frees every value at once.

## Changing a type

A type changes only in ways that keep every stored entry valid ([Content types](/reference/api/content-types/#changing-a-content-type) has the refusal's shape). With the newer types, the three groups are:

| Change | What happens |
|---|---|
| A new name, label or description; a new optional field, at any depth; a wider limit (`max_chars` up, `min_chars` down or removed, single-line to multi-line, a whole number to a decimal, `min` or `max` relaxed); more enum options; a larger `max` on a reference list, a list or blocks; a new kind of block; text becoming markdown with at least its `max_chars`; a pattern removed; `unique` dropped; `single` turned off | Always allowed. |
| Removing a field, making one required, a new required field, a narrower limit, removed enum options, a changed type (text becoming email, say), a new or changed pattern, a smaller `max`, a removed kind of block, one reference becoming a list or the reverse | Allowed if every current draft and every published version fits; otherwise `422` `needs_migration`, naming the entries in the way. |
| Making a field unique | Allowed if no two drafts and no two published entries share a value; otherwise `422` `needs_migration`. |
| Making a type single | Allowed if it has at most one entry; otherwise `422` `too_many_entries`. |
| A different `title_field`; a field turned into or out of a URL name; a reference pointed at another collection | Always `422` `needs_migration`, whatever is stored. |

A collection with more than 10,000 drafts and publications to check is refused without looking, `too_many_to_check`. Migrations with a plan, a dry run and per-entry outcomes, for the changes refused today, are planned. <Status planned="M.5" />

## A worked example

A blog's posts, with an author from a `people` collection, a code editors type by hand, tags, a hero and a page builder. Create it with [`POST {P}/content-types`](/reference/api/content-types/#post-pcontent-types) as `{"slug": "posts", "definition": …}`, after a `people` type exists for the references to name:

```json
{
  "name": "Posts",
  "title_field": "title",
  "fields": [
    { "id": "title", "label": "Title", "type": "text", "required": true, "max_chars": 120 },
    { "id": "slug", "label": "URL name", "type": "slug", "required": true, "source": "title" },
    { "id": "code", "label": "Post code", "description": "Three capital letters, a hyphen and four digits, like NEW-0042.", "type": "text", "max_chars": 8, "pattern": "[A-Z]{3}-[0-9]{4}", "unique": true },
    { "id": "author", "label": "Author", "type": "reference", "required": true, "to": "people" },
    { "id": "related", "label": "Related posts", "type": "reference", "to": "posts", "many": true, "max": 3 },
    { "id": "published_on", "label": "Date", "type": "date" },
    { "id": "tags", "label": "Tags", "type": "list", "of": { "type": "text", "max_chars": 30 }, "max": 10 },
    { "id": "hero", "label": "Hero", "type": "object", "fields": [
      { "id": "heading", "label": "Heading", "type": "text", "required": true },
      { "id": "accent", "label": "Accent colour", "type": "color" }
    ] },
    { "id": "sections", "label": "Sections", "type": "blocks", "max": 20, "blocks": [
      { "name": "text", "label": "Text", "fields": [
        { "id": "body", "label": "Body", "type": "markdown", "required": true }
      ] },
      { "name": "quote", "label": "Quote", "fields": [
        { "id": "text", "label": "Quote", "type": "text", "required": true, "multiline": true, "max_chars": 500 },
        { "id": "by", "label": "Said by", "type": "reference", "to": "people" }
      ] },
      { "name": "link", "label": "Link", "fields": [
        { "id": "url", "label": "Address", "type": "url", "required": true }
      ] }
    ] }
  ]
}
```

An entry, sent with its keys in any order:

```json
{
  "sections": [
    { "_block": "text", "body": "Colophon proves what it published.\n\n- receipts\n- releases" },
    { "text": "Ship it, then prove it.", "_block": "quote", "by": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d" },
    { "_block": "link", "url": "HTTPS://Example.COM/Docs" }
  ],
  "hero": { "accent": "#1a2b3c", "heading": "Spring launch" },
  "tags": ["launch", "news"],
  "title": "Spring launch",
  "slug": "spring-launch",
  "code": "NEW-0042",
  "author": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d",
  "published_on": "2026-11-03"
}
```

is stored as exactly these bytes: fields in definition order, `_block` first in each block, the address in its standard form, and `related`, left out, absent:

```json
{"title":"Spring launch","slug":"spring-launch","code":"NEW-0042","author":"0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d","published_on":"2026-11-03","tags":["launch","news"],"hero":{"heading":"Spring launch","accent":"#1a2b3c"},"sections":[{"_block":"text","body":"Colophon proves what it published.\n\n- receipts\n- releases"},{"_block":"quote","text":"Ship it, then prove it.","by":"0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d"},{"_block":"link","url":"https://example.com/Docs"}]}
```

What the server says to a few mistakes in that entry:

| Change to the entry | Status | `details.reason` | `details.pointer` |
|---|---|---|---|
| `"code": "new-0042"` | `422` | `pattern_mismatch` | `/code` |
| `"code": "NEW-٠٠٤٢"` (Arabic-Indic digits) | `422` | `pattern_mismatch` | `/code` |
| `"accent": "#1A2B3C"` in `hero` | `422` | `invalid_color` | `/hero/accent` |
| `"tags": []` | `422` | `invalid_list` | `/tags` |
| a second block with `"_block": "video"` | `422` | `unknown_block` | `/sections/1/_block` |
| an `author` id that names no person | `422` | `reference_not_found` | `/author` |
| a `code` another post already has | `409` | `value_taken` | `/code` |

Every row but the last two comes from the content type alone; those two come from the database. The canonical forms and the first five refusals above were produced by running this example through `ContentType::parse` and `validate`.

A site's settings, as a single type:

```json
{
  "name": "Site",
  "title_field": "site_name",
  "single": true,
  "fields": [
    { "id": "site_name", "label": "Site name", "type": "text", "required": true, "max_chars": 80 },
    { "id": "contact", "label": "Contact email", "type": "email" },
    { "id": "brand", "label": "Brand colour", "type": "color" }
  ]
}
```

Its first entry is created like any other; a second is `409` `single_exists`.

## How it is tested

- The unit tests in `crates/colophon-domain/src/schema.rs`: every type's values and refusals, the canonical forms and their round trips, definitions written before each new option keeping their bytes, structures and their depth, patterns that must match whole (and `(a+)+` against 50,000 characters finishing), and every kind of change.
- Against the real server and PostgreSQL: `crates/colophon-server/tests/references.rs` (references refused, listed, and kept whole by releases), `singles.rs` (two creations racing, one wins), `unique.rs` (drafts against publications, exact comparison, a type change refused then accepted, releases, who is named, racing writes) and `content_types.rs` (`a_pattern_is_checked_against_stored_content_and_holds_new_content_to_it`).
- The testbed's Modelling page drives the API in a browser; `perf/k6/modelling.js` is the load scenario, run as a smoke so far.

## Design

Decisions D-064 to D-067, D-069 and D-070, and the milestone M parts of modules 01 (domain) and 02 (db), are in `docs/` on the branch that builds them; the [design documents](/design/) here show them once it is merged.
