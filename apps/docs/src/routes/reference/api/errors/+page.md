<script>
	import Status from '#lib/components/Status.svelte';
</script>

Every error the server sends, from any route and from its middleware (a wrong method, an oversized body, a timeout, a panic), has the same shape. This page lists every `code`, every `details.reason` the code can produce, when each happens and which headers come with it, read from `crates/colophon-api/src/error.rs` and the handlers, the domain and the database crates. A test reads the same files and fails when a code or a reason is missing here.

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
- Reasons that depend on stored entries add `entries`, the entries in the way, and, for `needs_migration`, `total`, how many there are in all: `slug_taken`, `value_taken` and `needs_migration`. Each entry has `entry` (its id), `title`, `state` (`draft` or `published`) and, for `needs_migration`, `problem`: the `reason`, `pointer` and `message` its content would get. `slug_taken` and `value_taken` name an entry only to a role that may read its title, and never after a write lost a race to the database.
- Two reasons about a scheduled time add `candidates`, the two instants a local time could mean, earlier first, in RFC 3339: `ambiguous_local_time` and `skipped_local_time` ([Schedules](/reference/api/publish-requests/#schedules)).
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
| `unauthorized` | 401 | Not signed in, the session or token ended (12 hours after sign-in, 2 hours unused, revoked), a project API token was revoked or expired, or a bearer token isn't valid. |
| `forbidden` | 403 | Signed in, but not allowed: a role without the permission, a browser write without a trusted `Origin` or a matching CSRF token, a bearer token approving a device, or someone who isn't an administrator creating a project. `details.reason` says which ([Refusals](#refusals)). |
| `not_found` | 404 | No such resource, or one you may not see; the two look the same. Also an id that isn't a UUID, and an unknown, expired or used invitation, device code or device secret. |
| `gone` | 410 | Delivery only: it was published, and has been taken down. |
| `conflict` | 409 | The request conflicts with the current state; `details.reason` says how, [when it can](#conflicts). |
| `precondition_required` | 428 | A required `If-Match` or `Idempotency-Key` is missing; the message names it. |
| `route_not_found` | 404 | No route has this path. |
| `method_not_allowed` | 405 | The path exists, but not for this method. |
| `payload_too_large` | 413 | The body is over the limit (1 MiB by default, `COLOPHON_MAX_BODY_BYTES`), whether declared or streamed. |
| `unsupported_media_type` | 415 | The body needs `Content-Type: application/json`. |
| `validation_failed` | 422 | Well-formed, but breaks a rule; `details` names the rule and where. |
| `rate_limited` | 429 | Too many attempts lately; `Retry-After` says how many seconds to wait. Only setup codes are rate-limited today: five attempts a person and twenty in all per fifteen minutes ([`POST /api/v1/setup`](/reference/api/auth/#post-apiv1setup)). |
| `deadline_exceeded` | 504 | The response headers weren't ready within the request deadline (10 s by default, `COLOPHON_REQUEST_TIMEOUT_MS`). |
| `unavailable` | 503 | Temporarily unable to serve: no database connection came free within 2 s, so nothing was attempted (with `Retry-After: 1`); three device codes in a row clashed (with `Retry-After: 1`); or a dependency is missing or down: "this server has no database configured", "sign-in is not configured on this server", "the identity provider is unavailable". |
| `internal` | 500 | A bug or an unexpected failure, including a panic in a handler and a database statement over its 5-second limit. |

## Headers on errors

| Header | On |
|---|---|
| `x-request-id` | Every error, as on every response. |
| `ETag` | `409` `stale_revision`, `stale_version`, `stale_changeset` and `stale_role`: the current version, so a client can load it and try again. |
| `Colophon-Generation` | `409` `generation_moved`: the environment's generation now. |
| `Retry-After` | `503` when the server is at capacity: `1`, in seconds. `429` `rate_limited`: the seconds until there is room. |
| `Allow` | `405`: the methods the path has. |
| `Cache-Control: no-store` | `410`: if the entry is published again, no cache may still say it's gone. |
| `Connection: close` | Sometimes on `413` or another early refusal, when the unread body was over 32 MiB or took more than 2 s to drain. |

## Refusals

Every `403` has `details.reason`, a stable code to act on instead of the message, and `details.pointer`, empty. A missing permission also names it in `details.permission`, as roles write permissions:

```json
{"error":{"code":"forbidden","message":"your role can't publish this content","details":{"reason":"missing_permission","pointer":"","permission":"content.publish"},"request_id":"0193f6e2-8b9c-7d0e-9f1a-2b3c4d5e6f70"}}
```

| `details.reason` | Routes | When |
|---|---|---|
| `missing_permission` | any route a permission guards | Your role lacks the permission here, named in `details.permission` (`content.create`, `members.manage`, …). |
| `cannot_grant` | roles, members, invitations | Giving a role or a member powers your own role doesn't hold, or changing a member whose role has powers yours lacks. |
| `administrators_only` | `POST /api/v1/projects` | Only an administrator creates projects. |
| `not_set_up` | `POST /api/v1/projects` | The installation has no administrator yet: enter the setup code its server printed, or have its operator run `colophon admin bootstrap`. |
| `browser_session_required` | `POST /auth/device/approve` | A device is approved from a browser where you are signed in, never with a bearer token. |
| `origin_not_allowed` | any cookie write | The `Origin` header isn't a trusted origin. |
| `csrf_token_invalid` | any cookie write | The `X-CSRF-Token` header is missing or doesn't match the session's. |
| `titles_not_readable` | `GET {P}/collections/{collection}/entries?q=` | Your role can't read titles in this collection, not even on your own entries, so it can't search them. |
| `api_token_not_allowed` | every route but its project's routes under `{P}` and the rich-text validator; and `POST {P}/changesets/{changeset}/schedule` | The caller is a [project API token](/reference/api/tokens/), which works only with its project's content and never schedules. <Status building="R.4" /> |

## Conflicts

Every `409` carries a reason, and some a header naming the current state:

| `details.reason` | Routes | When | Also in the response |
|---|---|---|---|
| `stale_revision` | `PUT` an entry | The `If-Match` revision is no longer the draft. Nothing was written. | `ETag`: the draft's revision |
| `stale_version` | `PUT` a content type | The `If-Match` version is no longer current. | `ETag`: the current version |
| `stale_changeset` | `submit`, `refresh`, `reviews`, `publish`, `schedule` | The changeset no longer has the `If-Match` hash. | `ETag`: the current hash |
| `generation_moved` | `publish` | Another release committed first: the environment is past `base_generation`. | `Colophon-Generation`: the current generation |
| `idempotency_key_reused` | the six [keyed routes](/reference/api/#idempotency-keys) | The key was used before, by you, here, for a different request. | |
| `stale_role` | `PUT`, `DELETE` a role | The `If-Match` version is no longer current. | `ETag`: the current version |
| `role_in_use` | `DELETE` a role | Members hold the role or pending invitations offer it; the message says how many. | |
| `slug_taken` | create or save an entry; `publish` | Another entry of the collection has this URL name, as its draft or published. On a save, `pointer` is the slug field and `entries` names that entry; when a write lost a race to the database's unique index, or on publish, neither is known. | |
| `value_taken` | create or save an entry; `publish` | Another entry of the collection has this value of a [unique field](/reference/content-modelling/#unique-fields), as its draft or published. On a save, `pointer` is the field and `entries` names that entry, as for `slug_taken`; on publish, the release would put live a value another entry still publishes. <Status building="M.4" /> | |
| `single_exists` | create an entry | The collection is a [single type](/reference/content-modelling/#single-types) and has its entry. <Status building="M.4" /> | |
| `references_unpublished` | `publish` | The release would publish an entry naming an entry that isn't published and wouldn't be. <Status building="M.2" /> | |
| `still_referenced` | `publish` | The release would take down an entry a published entry still names. <Status building="M.2" /> | |
| `already_set_up` | `POST /api/v1/setup` | The installation has an administrator already, so setup codes do nothing; ask one to invite you to a project. | |
| `project_exists` | `POST /api/v1/projects` | A project with this slug exists. | |
| `content_type_exists` | `POST {P}/content-types` | A content type with this slug exists. | |
| `role_exists` | `POST /api/v1/projects/{project}/roles` | The project has a role with this key. | |
| `owner_role_fixed` | `PUT`, `DELETE` the `owner` role | The owner role always holds every permission. | |
| `last_owner` | `PUT`, `DELETE` a member | A project must keep at least one owner. | |
| `already_member` | `POST /api/v1/invitations/accept` | You are a member of this project already; the invitation stays unused. | |
| `not_submitted` | `reviews` | The changeset hasn't been submitted for review yet. | |
| `entries_moved_on` | `reviews`, `publish`, `schedule` | An entry moved on after the changeset pinned it: refresh it, and have it reviewed again. | |
| `already_published` | `publish`, `schedule` | The changeset has been published already. | |
| `approval_expired` | `publish`, `schedule` | The approval expired: it needs reviewing again. | |
| `not_approved` | `publish`, `schedule` | Only an approved changeset can be published or scheduled. | |
| `already_scheduled` | `schedule` | It has a pending schedule; cancel that one to choose another time. | |
| `schedule_published` | `DELETE {P}/schedules/{schedule}` | It has published already; cancelling never undoes a release. | |
| `schedule_blocked` | `DELETE {P}/schedules/{schedule}` | It was blocked when its time came; there is nothing to cancel. | |
| `schedule_cancelled` | `DELETE {P}/schedules/{schedule}` | It was cancelled already. | |

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
| `unknown_role` | `/role` | member `PUT`, invitation `POST` | The project has no role with that key. |
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
| `setup_code_invalid` | `/code` | `POST /api/v1/setup` | A setup code that is wrong, already used or expired: one answer for all three. |
| `invalid_changeset` | (empty) | `GET {P}/schedules` | `?changeset=` isn't a changeset id. |

### Fields a role may not touch

Since step R.3, from creating and saving an entry ([Entries](/reference/api/entries/#fields-and-own-entries)):

| `details.reason` | `pointer` | When |
|---|---|---|
| `field_not_permitted` | the field, `/price` | A create sets a field the role can't create; a save changes a field the role can't change, or sends a field the role can't read with a value other than the stored one. |

### Scheduling

From `POST {P}/changesets/{changeset}/schedule` ([Schedules](/reference/api/publish-requests/#schedules)), defined in `crates/colophon-domain/src/schedule.rs`:

| `details.reason` | `pointer` | When |
|---|---|---|
| `invalid_local_time` | `/local_time` | Not a date and time like `2026-11-03T09:00`. |
| `unknown_time_zone` | `/time_zone` | Not an IANA time zone, like `Europe/London`. |
| `invalid_choice` | `/choice` | Neither `earlier` nor `later`. |
| `ambiguous_local_time` | `/local_time` | The clocks show that time twice in that zone, as they go back; `candidates` has both instants. |
| `skipped_local_time` | `/local_time` | The clocks skip that time in that zone, as they go forward; `candidates` has the two instants it could mean. |
| `in_the_past` | `/local_time` | Not after now. |
| `too_far_ahead` | `/local_time` | More than a year and a day (366 days) ahead. |
| `after_approval_expires` | `/local_time` | At or after the moment the approval it publishes under expires. |

### Roles

From `POST` and `PUT` on `/api/v1/projects/{project}/roles` ([Roles](/reference/api/roles/)):

| `details.reason` | `pointer` | When |
|---|---|---|
| `invalid_role_key` | `/key` | Not 1 to 63 lowercase letters, digits and hyphens starting with a letter. |
| `invalid_role_name` | `/name` | Blank, over 100 characters, or with control characters. |
| `invalid_role_description` | `/description` | Over 500 characters, or with control characters. |
| `too_many_permissions` | `/permissions` | More than 500. |
| `unknown_action` | `/permissions` | An action that isn't one of the twelve (eleven before R.4 added `audit.read`); the message names the permission's position. |
| `invalid_subject` | `/permissions` | A subject that is neither `*` nor a collection slug. |
| `project_wide_action` | `/permissions` | `members.manage`, `roles.manage`, `destinations.manage`, `tokens.manage` or `audit.read` on one collection. |
| `fields_not_applicable` | `/permissions` | `fields` on an action other than `content.read`, `content.create` or `content.update`. |
| `invalid_fields` | `/permissions` | `fields` is empty, has more than 100 ids, or names something that isn't a field id. (Also a definition with too few or too many fields, below.) |
| `unknown_condition` | `/permissions` | A `condition` that isn't `own`. |
| `condition_not_applicable` | `/permissions` | `own` on an action other than `content.read`, `content.update` or `content.delete`. |
| `too_many_roles` | `/key` | The project has 100 roles already. |

### API tokens

From `POST /api/v1/projects/{project}/tokens` ([API tokens](/reference/api/tokens/#post-apiv1projectsprojecttokens)); its permissions can also be refused with any of the role codes above. <Status building="R.4" />

| `details.reason` | `pointer` | When |
|---|---|---|
| `invalid_token_name` | `/name` | Blank, over 100 characters, or with control characters. |
| `invalid_token_description` | `/description` | Over 500 characters, or with control characters. |
| `unknown_token_kind` | `/kind` | Not `read-only`, `full-access` or `custom`. |
| `invalid_expiry` | `/expires_in_days` | Outside 1 to 366. |
| `permissions_required` | `/permissions` | A `custom` token without permissions. |
| `permissions_not_applicable` | `/permissions` | Permissions on a `read-only` or `full-access` token, whose permissions are fixed. |
| `not_for_tokens` | `/permissions` | `members.manage`, `roles.manage`, `tokens.manage` or `audit.read`: those stay with people. |
| `too_many_tokens` | (empty) | The project has 100 active tokens already. |

### Content and content types

From creating or saving an entry (the pointer is into the content) and from creating or changing a content type (the pointer is into the definition, under `/definition` when creating). [Content types](/reference/api/content-types/) has the rules.

| `details.reason` | In | When |
|---|---|---|
| `not_an_object` | both | The content, the definition or a field definition isn't a JSON object. |
| `unknown_field` | both | A key the content type, the definition or the field's type doesn't have. Never dropped silently. |
| `missing_field` | both | A required field is missing from content; `fields`, `title_field` or an enum's `values` from a definition. |
| `wrong_type` | both | A value of the wrong JSON type: text that isn't a string, a number that isn't one, a list or page builder that isn't a list, `required`, `multiline`, `unique` or `single` that isn't `true` or `false`. |
| `out_of_range` | both | A number outside its field's `min` and `max`; in a definition, `max_chars` outside 1 to 100,000, `min_chars` outside 1 to `max_chars`, a `min` or `max` the field couldn't hold, a `max` below `min`, or a list's, page builder's or reference list's `max` outside 1 to 100. |
| `too_long` | content | Text or Markdown over its field's `max_chars`, or JSON over 64 KiB. |
| `too_short` | content | Text under its field's `min_chars`. <Status building="M.1" /> |
| `pattern_mismatch` | content | Text that doesn't match its field's `pattern` whole. <Status building="M.4" /> |
| `invalid_email` | content | Not an address of the form `local@domain` with a dot in the domain. <Status building="M.1" /> |
| `invalid_url` | content | Not an `http` or `https` address with a host and no credentials, or over 2,048 characters. (Also a website connection's address, above.) <Status building="M.1" /> |
| `invalid_date` | content | Not a real calendar date written `YYYY-MM-DD`. <Status building="M.1" /> |
| `invalid_time` | content | Not a time written `HH:MM` or `HH:MM:SS`, 24-hour. <Status building="M.1" /> |
| `invalid_color` | content | Not `#rrggbb` in lowercase. <Status building="M.1" /> |
| `invalid_json` | content | A JSON value nested more than 32 levels, or holding a number outside the `number` rules. <Status building="M.1" /> |
| `invalid_reference` | content | A reference that isn't an entry id in lowercase, or an empty list of them. <Status building="M.2" /> |
| `duplicate_reference` | content | A list of references that names one entry twice. <Status building="M.2" /> |
| `reference_not_found` | content | A reference names no entry of its field's collection; found by the database when the version is stored. <Status building="M.2" /> |
| `invalid_list` | content | An empty list or page builder: leave the field out instead. <Status building="M.3" /> |
| `too_many` | content | More items in a list, page builder or reference list than its `max`. <Status building="M.2" /> |
| `unknown_block` | content | A page-builder item whose `_block` is missing or names no kind of block the field offers; the pointer ends in `/_block`. <Status building="M.3" /> |
| `invalid_text` | content | Empty text (leave the field out instead), control characters, or, in single-line text, spaces at either end. |
| `not_an_integer` | content | A number in a whole-number field that isn't a whole number within ±9,007,199,254,740,991. |
| `too_precise` | content | A decimal with more than 6 places, or 1,000,000,000 or more in size. |
| `not_an_option` | content | A value that isn't one of the enum's options. |
| `invalid_datetime` | content | Not a real UTC time written `YYYY-MM-DDTHH:MM:SSZ`. |
| `invalid_slug` | content | A URL name that isn't 1 to 120 lowercase letters and digits in groups joined by single hyphens. |
| `invalid_name` | definition | The content type's name: 1 to 100 characters, no control characters, no spaces at either end. |
| `invalid_fields` | definition | Fewer than 1 or more than 100 fields. |
| `duplicate_field` | definition | Two fields with one id. |
| `invalid_id` | definition | A field id that isn't 1 to 64 lowercase letters, digits and underscores, starting with a letter. |
| `invalid_label` | definition | A label that isn't 1 to 100 characters without control characters or spaces at either end. |
| `invalid_type` | definition | A `type` that isn't one of the [field types](/reference/content-modelling/#field-types); a URL name inside a structure; or a list whose items are lists or blocks. |
| `invalid_values` | definition | Enum options: none, more than 100, duplicated, or badly written. |
| `invalid_title_field` | definition | `title_field` doesn't name a required, single-line text field. |
| `duplicate_slug` | definition | A second slug field. |
| `invalid_source` | definition | A slug field's `source` that isn't a field id, or doesn't name a single-line text field of the type. |
| `invalid_description` | definition | A field's description over 500 characters, or with control characters or spaces at either end. <Status building="M.1" /> |
| `invalid_target` | definition | A reference field's `to` that isn't a collection slug. <Status building="M.2" /> |
| `too_deep` | definition | Structures nested more than three deep. (Also a rich-text code.) <Status building="M.3" /> |
| `invalid_blocks` | definition | A page builder offering no kinds of block, or more than 20. <Status building="M.3" /> |
| `duplicate_block` | definition | Two kinds of block with one name. <Status building="M.3" /> |
| `invalid_pattern` | definition | A `pattern` that isn't 1 to 500 characters, doesn't compile in Rust `regex` syntax (look-around and back-references don't), or compiles too large; the pointer ends in `/pattern`. <Status building="M.4" /> |
| `invalid_unique` | definition | `unique` on a field that can't be unique: anything but a top-level single-line text, email, URL, number, date, time or date-time field. <Status building="M.4" /> |

A rich-text field's value can also be refused with any [rich-text code](#rich-text), its pointer starting at the field.

### Changing a content type

| `details.reason` | When | Extra details |
|---|---|---|
| `needs_migration` | The change would make stored content invalid, or always needs a migration: a new `title_field`, or a field turned into or out of a slug field. | When stored entries are in the way: `entries` (the first 20 drafts or publications) and `total`. Each entry's `problem.reason` is a content code from the table above, `value_taken` when a field being made unique shares its value with another entry's, or `unreadable` if its stored content couldn't be read as JSON. |
| `too_many_to_check` | The change needs checking against more than 10,000 drafts and publications. | |
| `too_many_entries` | `single` turned on while the collection has more than one entry; the pointer is `/single`. <Status building="M.4" /> | |

### Rich text

From `POST /api/v1/rich-text/validate` and from any rich-text field, with a pointer into the document: `unsupported_schema`, `unsupported_version`, `wrong_type`, `missing_field`, `unknown_field`, `unknown_node`, `invalid_content`, `unknown_mark`, `invalid_marks`, `invalid_attribute`, `unsafe_link`, `empty_text`, `too_deep`, `too_large` and `too_many_nodes`. The [rich-text format](/reference/rich-text/#error-codes) says what each means.

## Not built yet

- Rate limits on other routes, per person or per token; today only setup codes have one. <Status planned="S" />
