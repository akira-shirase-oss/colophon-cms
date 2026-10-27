<script>
	import Status from '#lib/components/Status.svelte';
</script>

Every error the server sends, from any route and from its middleware (a wrong method, an oversized body, a timeout, a panic), has the same shape. This page lists every `code`, every `details.reason` the code can produce, when each happens and which headers come with it, read from `crates/colophon-api/src/error.rs` and the handlers, the domain and the database crates on the newest branch, `r/r.2-custom-roles`. Reasons that exist only on a branch carry its label.

## The envelope

```json
{
  "error": {
    "code": "conflict",
    "message": "the draft has changed since that revision; load the current one and try again",
    "details": { "reason": "stale_revision", "pointer": "" },
    "request_id": "0192f3c4-7d1e-7a51-9c2b-5d3e8f6a1b20"
  }
}
```

- **Branch on `code`**, and on `details.reason` when there is one. Never on `message`: it is for people and may change.
- `details` appears when the error is about part of the request: `reason` is a specific code, and `pointer` is an [RFC 6901 JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901) into the body (empty for the whole body or none).
- Two reasons that depend on stored entries add `entries`, the entries in the way, and `total`, how many there are in all: `slug_taken` and `needs_migration` <Status building="3.6" />. Each entry has `entry` (its id), `title`, `state` (`draft` or `published`) and, for `needs_migration`, `problem`: the `reason`, `pointer` and `message` its content would get.
- `request_id` matches the `x-request-id` header. Quote it in a bug report and the logs find the request.
- A message never shows an internal cause: a `500` says `internal error`, and the cause goes to the server's log. A message that repeats a client's input (a JSON parser's complaint) is cut at 200 characters.
- Errors are logged once, at a level that matches who is at fault: client mistakes at debug, a full pool at warn, internal errors at error.

## Codes

Renaming a code is a breaking change. They are defined in `ErrorCode` in `crates/colophon-api/src/error.rs`, and a test checks that every one is listed here.

| Code | Status | Meaning |
|---|---|---|
| `bad_request` | 400 | A malformed request that no more specific code describes: a malformed `If-Match` or `Idempotency-Key`, a `limit` outside 1 to 100, a cursor from another list, a title search over 100 characters, a query parameter or path segment that doesn't parse (`?generation=ten`, `/releases/latest`), or a body that couldn't be read. |
| `malformed_json` | 400 | The body isn't valid JSON, or doesn't have the expected shape: a required field missing, a value of the wrong type, or an unknown field in a body with a fixed shape. The message quotes the parser. |
| `sign_in_failed` | 400 | Sign-in failed, was cancelled, or its attempt expired or was already used. Details are logged, never shown. |
| `unauthorized` | 401 | Not signed in, the session or token ended (12 hours after sign-in, 2 hours unused, revoked), or a bearer token isn't valid. |
| `forbidden` | 403 | Signed in, but not allowed: a role without the permission, a browser write without a trusted `Origin` or a matching CSRF token, a bearer token approving a device, or someone who isn't an administrator creating a project. The message says which. |
| `not_found` | 404 | No such resource, or one you may not see; the two look the same. Also an id that isn't a UUID, and an unknown, expired or used invitation, device code or device secret. |
| `gone` | 410 | Delivery only: it was published, and has been taken down. |
| `conflict` | 409 | The request conflicts with the current state; `details.reason` says how, [when it can](#conflicts). |
| `precondition_required` | 428 | A required `If-Match` or `Idempotency-Key` is missing; the message names it. |
| `route_not_found` | 404 | No route has this path. |
| `method_not_allowed` | 405 | The path exists, but not for this method. |
| `payload_too_large` | 413 | The body is over the limit (1 MiB by default, `COLOPHON_MAX_BODY_BYTES`), whether declared or streamed. |
| `unsupported_media_type` | 415 | The body needs `Content-Type: application/json`. |
| `validation_failed` | 422 | Well-formed, but breaks a rule; `details` names the rule and where. |
| `deadline_exceeded` | 504 | The response headers weren't ready within the request deadline (10 s by default, `COLOPHON_REQUEST_TIMEOUT_MS`). |
| `unavailable` | 503 | Temporarily unable to serve: no database connection came free within 2 s, so nothing was attempted (with `Retry-After: 1`); three device codes in a row clashed (with `Retry-After: 1`); or a dependency is missing or down: "this server has no database configured", "sign-in is not configured on this server", "the identity provider is unavailable". |
| `internal` | 500 | A bug or an unexpected failure, including a panic in a handler and a database statement over its 5-second limit. |

## Headers on errors

| Header | On |
|---|---|
| `x-request-id` | Every error, as on every response. |
| `ETag` | `409` `stale_revision`, `stale_version`, `stale_changeset` and `stale_role`: the current version, so a client can load it and try again. |
| `Colophon-Generation` | `409` `generation_moved`: the environment's generation now. |
| `Retry-After` | `503` when the server is at capacity: `1`, in seconds. |
| `Allow` | `405`: the methods the path has. |
| `Cache-Control: no-store` | `410`: if the entry is published again, no cache may still say it's gone. |
| `Connection: close` | Sometimes on `413` or another early refusal, when the unread body was over 32 MiB or took more than 2 s to drain. |

## Conflicts

A `409` carries a reason when the client can act on it, and a header naming the current state:

| `details.reason` | Routes | When | Also in the response |
|---|---|---|---|
| `stale_revision` | `PUT` an entry | The `If-Match` revision is no longer the draft. Nothing was written. | `ETag`: the draft's revision |
| `stale_version` | `PUT` a content type | The `If-Match` version is no longer current. | `ETag`: the current version |
| `stale_changeset` | `submit`, `refresh`, `reviews`, `publish` | The changeset no longer has the `If-Match` hash. | `ETag`: the current hash |
| `generation_moved` | `publish` | Another release committed first: the environment is past `base_generation`. | `Colophon-Generation`: the current generation |
| `idempotency_key_reused` | the six [keyed routes](/reference/api/#idempotency-keys) | The key was used before, by you, here, for a different request. | |
| `stale_role` <Status building="R.2" /> | `PUT`, `DELETE` a role | The `If-Match` version is no longer current. | `ETag`: the current version |
| `role_in_use` <Status building="R.2" /> | `DELETE` a role | Members hold the role or pending invitations offer it; the message says how many. | |
| `slug_taken` <Status building="3.6" /> | create or save an entry; `publish` | Another entry of the collection has this URL name, as its draft or published. On a save, `pointer` is the slug field and `entries` names that entry; when a write lost a race to the database's unique index, or on publish, neither is known. | |

Other conflicts have no reason, only a message:

| Route | Message |
|---|---|
| `POST /api/v1/projects` | a project with this slug already exists |
| `POST {P}/content-types` | a content type with this slug already exists |
| `POST /api/v1/projects/{project}/roles` <Status building="R.2" /> | this project already has a role with that key |
| `PUT`, `DELETE` the `owner` role <Status building="R.2" /> | the owner role is fixed: it always holds every permission |
| `PUT`, `DELETE` a member | a project must keep at least one owner |
| `POST /api/v1/invitations/accept` | you are already a member of this project |
| `POST {P}/changesets/{changeset}/reviews` | this changeset hasn't been submitted for review yet; or an entry moved on after this changeset pinned it, so refresh it first |
| `POST {P}/changesets/{changeset}/publish` | already published; an entry moved on (refresh and review again); the approval has expired; or only an approved changeset can be published |

## Validation reasons

A `422` `validation_failed` always has `details` with a `reason` and a `pointer`.

### Request fields

| `details.reason` | `pointer` | Route | When |
|---|---|---|---|
| `invalid_client_name` | `/client_name` | `POST /auth/device` | Blank, over 100 characters, or with control characters. |
| `invalid_slug` | `/slug` | `POST /api/v1/projects`, `POST {P}/content-types` | Not 1 to 63 lowercase letters, digits and hyphens, or starting or ending with a hyphen. (Also a URL name in content, below.) |
| `invalid_name` | `/name` | `POST /api/v1/projects` (1 to 200 characters), `POST {P}/destinations` (1 to 100) | Blank, too long, or with control characters. (Also a definition's name, below.) |
| `reserved_slug` | `/slug` | `POST {P}/content-types` | `members`, `settings`, `changesets` or `releases`, which name Studio pages. |
| `invalid_role` | `/role` | member `PUT`, invitation `POST` | Not shaped like a role key. |
| `unknown_role` <Status building="R.2" /> | `/role` | member `PUT`, invitation `POST` | The project has no role with that key. |
| `invalid_label` | `/label` | invitation `POST` | Blank, over 100 characters, or with control characters. (Also a field's label in a definition.) |
| `invalid_title` | `/title` | `POST {P}/changesets` | Blank, over 200 characters, or with control characters. |
| `invalid_entries` | `/entries` | `POST {P}/changesets` | No entries, more than 100, or one listed twice. |
| `unknown_entry` | `/entries`, `/unpublish` | `POST {P}/changesets` | An id isn't a UUID; or, in `/entries`, isn't an entry in this environment. |
| `not_published` | `/unpublish` | `POST {P}/changesets` | An entry to take down isn't published here. |
| `invalid_decision` | `/decision` | `POST …/reviews` | Not `approve` or `request_changes`. |
| `invalid_comment` | `/comment` | `POST …/reviews` | Over 2,000 characters. |
| `invalid_generation` | `/base_generation` | `POST …/publish` | Negative. |
| `invalid_generation` | `/generation` | `/content/v1` | A release that hasn't committed: outside 1 to the latest generation. |
| `invalid_kind` | `/kind` | `POST {P}/destinations` | Not `webhook`, `cdn` or `probe`. |
| `invalid_location` | `/location` | `POST {P}/destinations` | A location on a kind other than `probe`, or not 1 to 63 lowercase letters, digits and hyphens. |
| `invalid_url` | `/url` | `POST {P}/destinations` | Over 2,000 characters, not a URL, not `https` (outside development), with credentials, or a private IP address. |

### Roles

From `POST` and `PUT` on `/api/v1/projects/{project}/roles` ([Roles](/reference/api/roles/)) <Status building="R.2" />:

| `details.reason` | `pointer` | When |
|---|---|---|
| `invalid_role_key` | `/key` | Not 1 to 63 lowercase letters, digits and hyphens starting with a letter. |
| `invalid_role_name` | `/name` | Blank, over 100 characters, or with control characters. |
| `invalid_role_description` | `/description` | Over 500 characters, or with control characters. |
| `too_many_permissions` | `/permissions` | More than 500. |
| `unknown_action` | `/permissions` | An action that isn't one of the eleven; the message names the permission's position. |
| `invalid_subject` | `/permissions` | A subject that is neither `*` nor a collection slug. |
| `project_wide_action` | `/permissions` | `members.manage`, `roles.manage`, `destinations.manage` or `tokens.manage` on one collection. |
| `not_yet_enforced` | `/permissions` | A permission with `fields` or `condition`, refused until step R.3 enforces them. |
| `too_many_roles` | `/key` | The project has 100 roles already. |

### Content and content types

From creating or saving an entry (the pointer is into the content) and from creating or changing a content type (the pointer is into the definition, under `/definition` when creating). [Content types](/reference/api/content-types/) has the rules.

| `details.reason` | In | When |
|---|---|---|
| `not_an_object` | both | The content, the definition or a field definition isn't a JSON object. |
| `unknown_field` | both | A key the content type, the definition or the field's type doesn't have. Never dropped silently. |
| `missing_field` | both | A required field is missing from content; `fields`, `title_field` or an enum's `values` from a definition. |
| `wrong_type` | both | A value of the wrong JSON type: text that isn't a string, a number that isn't one, `required` or `multiline` that isn't `true` or `false`. |
| `out_of_range` | both | A number outside its field's `min` and `max`; in a definition, `max_chars` outside 1 to 100,000, or a `min` or `max` the field couldn't hold, or a `max` below `min`. |
| `too_long` | content | Text over its field's `max_chars`. |
| `invalid_text` | content | Empty text (leave the field out instead), control characters, or, in single-line text, spaces at either end. |
| `not_an_integer` | content | A number in a whole-number field that isn't a whole number within ±9,007,199,254,740,991. |
| `too_precise` | content | A decimal with more than 6 places, or 1,000,000,000 or more in size. |
| `not_an_option` | content | A value that isn't one of the enum's options. |
| `invalid_datetime` | content | Not a real UTC time written `YYYY-MM-DDTHH:MM:SSZ`. |
| `invalid_slug` <Status building="3.6" /> | content | A URL name that isn't 1 to 120 lowercase letters and digits in groups joined by single hyphens. |
| `invalid_name` | definition | The content type's name: 1 to 100 characters, no control characters, no spaces at either end. |
| `invalid_fields` | definition | Fewer than 1 or more than 100 fields. |
| `duplicate_field` | definition | Two fields with one id. |
| `invalid_id` | definition | A field id that isn't 1 to 64 lowercase letters, digits and underscores, starting with a letter. |
| `invalid_label` | definition | A label that isn't 1 to 100 characters without control characters or spaces at either end. |
| `invalid_type` | definition | A `type` that isn't `text`, `rich_text`, `number`, `boolean`, `enum`, `datetime` or (3.6) `slug`. |
| `invalid_values` | definition | Enum options: none, more than 100, duplicated, or badly written. |
| `invalid_title_field` | definition | `title_field` doesn't name a required, single-line text field. |
| `duplicate_slug` <Status building="3.6" /> | definition | A second slug field. |
| `invalid_source` <Status building="3.6" /> | definition | A slug field's `source` that isn't a field id, or doesn't name a single-line text field of the type. |

A rich-text field's value can also be refused with any [rich-text code](#rich-text), its pointer starting at the field.

### Changing a content type

| `details.reason` | When | Extra details |
|---|---|---|
| `needs_migration` | The change would make stored content invalid, or always needs a migration: a new `title_field`, or a field turned into or out of a slug field. On `main`, every narrowing change. | Since 3.6, when stored entries are in the way: `entries` (the first 20 drafts or publications) and `total`. Each entry's `problem.reason` is a content code from the table above, or `unreadable` if its stored content couldn't be read as JSON <Status building="3.6" />. |
| `too_many_to_check` <Status building="3.6" /> | The change needs checking against more than 10,000 drafts and publications. | |

### Rich text

From `POST /api/v1/rich-text/validate` and from any rich-text field, with a pointer into the document: `unsupported_schema`, `unsupported_version`, `wrong_type`, `missing_field`, `unknown_field`, `unknown_node`, `invalid_content`, `unknown_mark`, `invalid_marks`, `invalid_attribute`, `unsafe_link`, `empty_text`, `too_deep`, `too_large` and `too_many_nodes`. The [rich-text format](/reference/rich-text/#error-codes) says what each means.

## Not built yet

- `field_not_permitted`: a write that changes a field the caller's role may not change, with a pointer to it. <Status planned="R.3" />
- `429` and a rate limit per person or per token. <Status planned="not scheduled" />
