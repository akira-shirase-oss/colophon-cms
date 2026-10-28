<script>
	import Status from '#lib/components/Status.svelte';
</script>

An **API token** lets a script, a build or an agent use the API without a person signing in: it belongs to one project, holds permissions the way a role does, and acts as an identity of its own, so what it writes is attributed to the token, not to whoever made it. The **audit log** records every change to who may do what in a project: roles, memberships, invitations and tokens. Both are step R.4 (D-063, migration 0023), built on a branch with their tests, a testbed page and a load scenario (`perf/k6/tokens.js`) whose million-request runs haven't been made yet; nothing here is merged. This page also documents [`GET {P}/permissions`](#get-ppermissions), which tells any caller, a person or a token, what it may do. Handlers: `crates/colophon-api/src/tokens.rs`, `audit.rs`, and `roles.rs` for the permissions route; sign-in by token is in `session.rs`.

The Studio has no page for tokens or the audit log yet: owners make and revoke tokens through these routes, or on the testbed's API tokens page.

## What a token may do

A token is one of three kinds, chosen when it is made and fixed after:

| `kind` | Permissions |
|---|---|
| `read-only` | `content.read` on every collection. |
| `full-access` | Every action a token may hold, on every collection: `content.read`, `.create`, `.update`, `.delete`, `.review` and `.publish`, `schema.change` and `destinations.manage`. |
| `custom` | The permissions listed when it is made, written like a [role's](/reference/api/roles/#permissions), narrowed to fields or to the token's own entries if you like. |

- **Never access management, never the audit log.** No token may hold `members.manage`, `roles.manage`, `tokens.manage` or `audit.read`, whatever its kind; asking for one is `422` `not_for_tokens`, and the database refuses them too. People keep the actions that change who may do what and read the record of it, so a leaked token can neither widen anyone's power nor read or hide what it did.
- **Nobody makes a token more powerful than themselves.** The maker's role, as it is at that moment, must cover every permission the token gets; otherwise `403` `cannot_grant`. A `full-access` token therefore needs a role that holds all eight actions above.
- **Only its own project's content.** A token works on its project's routes under `{P}` (`/api/v1/projects/{project}/environments/{environment}/…`) and on `POST /api/v1/rich-text/validate`. Another project's routes answer `404`, as projects a person isn't in do. Everything else, such as `/me`, the project list, members, roles, tokens and the audit log, answers `403` `api_token_not_allowed`.
- **No schedules.** `POST {P}/changesets/{changeset}/schedule` is `403` `api_token_not_allowed`: a schedule publishes later as the person who made it, and a token isn't one.
- **Its own identity.** A token acts as an identity of its own (`identity_id` below), which no sign-in provider uses. What it creates names that identity in `created_by` and `author_id`, and the audit log names it as the actor.

## Using a token

Send the secret as a bearer token:

```http
GET /api/v1/projects/0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f/environments/production/collections/articles/entries HTTP/1.1
Authorization: Bearer colophon_api_…
```

- **The secret** is `colophon_api_` followed by 43 random characters, 56 in all. The prefix keeps it apart from a person's command-line token and lets secret scanners recognise one that leaked. It is shown once, in the answer that makes the token; the server keeps only its SHA-256 hash.
- **No CSRF token, no `Origin`.** As with any bearer token, a request with `Authorization` is judged by that header alone, and a token that is wrong, revoked or expired is `401`, never a fall back to a cookie.
- **Revoking is exact.** Each request re-checks the token inside its project and holds a share lock on its row until the request's transaction ends; revoking takes the row's update lock. So a revocation waits for the requests already running under the token, and none starts once it has committed: the next one is `401` (D-063; `a_token_does_exactly_what_its_permissions_allow_and_stops_when_revoked`).
- **Expiry.** A token expires 1 to 366 days after it is made, a year and a day at most so a yearly rotation never lapses. There is no token without an expiry.
- Its last use is written at most once a minute, in a transaction of its own, so requests never queue on it.

## The token object

```json
{
  "id": "0193f7a1-3c2d-7e4f-8a5b-6c7d8e9f0a1b",
  "identity_id": "0193f7a1-3c2d-7e4f-8a5b-6c7d8e9f0a1c",
  "name": "Site build",
  "description": "Reads published and draft content for the preview build.",
  "kind": "read-only",
  "permissions": [ { "action": "content.read", "subject": "*" } ],
  "state": "active",
  "created_by": { "id": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d", "name": "Ada Owner" },
  "created_at": "2026-10-28T09:00:00.120344Z",
  "expires_at": "2026-11-27T09:00:00.120344Z",
  "last_used_at": null,
  "revoked_at": null
}
```

| Field | Meaning |
|---|---|
| `id` | The token's id, `{token}` in paths. |
| `identity_id` | The identity it acts as: what it writes names this id. |
| `name`, `description` | What it is for, for people. |
| `kind` | `read-only`, `full-access` or `custom`. |
| `permissions` | What it may do, as roles show permissions: `{"action", "subject"}`, with `fields` and `condition` where they narrow it. |
| `state` | `active`, `expired` or `revoked`. |
| `created_by` | Who made it: an identity id and a display name. |
| `created_at`, `expires_at` | When it was made; when it stops working. |
| `last_used_at` | Its last use, to the minute; `null` before the first. |
| `revoked_at` | When it was revoked, or `null`. |
| `secret` | Only in the answer to `POST`: the secret itself. Never listed, never logged, never in the audit log. |

## Routes

### `GET /api/v1/projects/{project}/tokens`

The project's tokens, active, expired and revoked, newest first.

- **Who:** `tokens.manage` (owners), checked against your role as it is now. Not an API token.

**Response:** `200`, at most 200, on one page (`next_cursor` always `null`), without secrets.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | `missing_permission` | Your role lacks `tokens.manage`. |
| `403` | `forbidden` | `api_token_not_allowed` | The caller is an API token. |
| `404` | `not_found` | | No such project, or you aren't a member. |

### `POST /api/v1/projects/{project}/tokens`

Makes a token and answers its secret, once.

- **Who:** `tokens.manage` (owners), and a role that covers every permission the token gets.
- **Body:**

```json
{
  "name": "Importer",
  "description": "Nightly import from the old site.",
  "kind": "custom",
  "permissions": [
    { "action": "content.read", "subject": "articles" },
    { "action": "content.create", "subject": "articles" }
  ],
  "expires_in_days": 30
}
```

| Field | Type | Rules |
|---|---|---|
| `name` | string | Required. 1 to 100 characters without control characters, trimmed. |
| `description` | string | Optional. At most 500 characters without control characters, trimmed. |
| `kind` | string | Required. `read-only`, `full-access` or `custom`. |
| `permissions` | array | Required for `custom`, with at least one permission, written as for [roles](/reference/api/roles/#permissions); refused for the other two kinds, whose permissions are fixed. |
| `expires_in_days` | integer | Required. 1 to 366. |

Any other field is `400` `malformed_json`.

**Response:** `201 Created`, `{"data": …}` with [the token](#the-token-object) and its `secret`. Store the secret now: no route shows it again.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `malformed_json` | | An unknown field, or a value of the wrong type. |
| `403` | `forbidden` | `missing_permission` | Your role lacks `tokens.manage`. |
| `403` | `forbidden` | `cannot_grant` | The token would get a permission your role doesn't hold. |
| `403` | `forbidden` | `api_token_not_allowed` | The caller is an API token. |
| `422` | `validation_failed` | `invalid_token_name` | `/name` is blank, over 100 characters, or has control characters. |
| `422` | `validation_failed` | `invalid_token_description` | `/description` is over 500 characters or has control characters. |
| `422` | `validation_failed` | `unknown_token_kind` | `/kind` isn't one of the three. |
| `422` | `validation_failed` | `invalid_expiry` | `/expires_in_days` is outside 1 to 366. |
| `422` | `validation_failed` | `permissions_required` | A `custom` token without permissions. |
| `422` | `validation_failed` | `permissions_not_applicable` | Permissions on a `read-only` or `full-access` token. |
| `422` | `validation_failed` | `not_for_tokens` | `/permissions` asks for `members.manage`, `roles.manage`, `tokens.manage` or `audit.read`. |
| `422` | `validation_failed` | a [role permission code](/reference/api/errors/#roles) | A permission that is malformed: an unknown action, a bad subject, fields or a condition where they don't apply. |
| `422` | `validation_failed` | `too_many_tokens` | The project has 100 active tokens; revoke one first. |

### `DELETE /api/v1/projects/{project}/tokens/{token}`

Revokes a token. Requests already running under it finish first; none starts after. A revoked token is kept, never deleted, so what it did stays attributed.

- **Who:** `tokens.manage` (owners). Not an API token.

**Response:** `200`, `{"data": …}` with the token, now `revoked`. Revoking it again changes nothing and answers it as it is, with the same `revoked_at`.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | `missing_permission` | Your role lacks `tokens.manage`. |
| `403` | `forbidden` | `api_token_not_allowed` | The caller is an API token. |
| `404` | `not_found` | | No such token in this project. |

### `GET {P}/permissions`

What the caller may do in this project: a person's role and its permissions, or a token's own. Clients use it to offer only what will work; the [MCP server](/reference/mcp/) lists its tools from it. It is discovery, not authorization: a permission narrowed to some fields or to the caller's own entries still refuses other entries, and every request is still decided on its own.

- **Who:** any member, and any API token of the project. It sits under `{P}`, so a revoked token gets `401` and another project's token `404`.

**Response:** `200`:

```json
{
  "data": {
    "kind": "token",
    "role": null,
    "permissions": [
      { "action": "content.create", "subject": "articles" },
      { "action": "content.read", "subject": "articles" }
    ]
  }
}
```

| Field | Meaning |
|---|---|
| `kind` | `person` or `token`. |
| `role` | A person's role key, such as `owner`; `null` for a token. |
| `permissions` | As roles show them, sorted: `{"action", "subject"}`, with `fields` and `condition` where they narrow it. |

| Status | `code` | When |
|---|---|---|
| `401` | `unauthorized` | Not signed in, or the token is wrong, revoked or expired. |
| `404` | `not_found` | No such project or environment, or you aren't a member. |

## The audit log

Every change to roles, memberships, invitations and tokens is written to `audit_events` **in the same transaction as the change**, so a change and its record commit together or not at all.

- **Append-only, for everyone.** The API's database role may only insert and read the table, and a trigger refuses `UPDATE`, `DELETE` and `TRUNCATE` from anyone, the table's owner included (`crates/colophon-db/tests/tokens.rs`).
- **Never a secret.** No token's secret and no invitation link is recorded.
- **Owners read it.** Reading needs `audit.read`, which every owner holds and no API token can.
- **Kept for now.** Nothing deletes events yet. Retention, 90 days by default, arrives with operations as a separate, audited purge. <Status planned="P6" />

| `action` | `subject` | `detail` |
|---|---|---|
| `role.created` | the role's key | its `name`, `description` and `permissions` |
| `role.updated` | the role's key | `before` and `after`, each as above |
| `role.deleted` | the role's key | the role as it was |
| `member.role_changed` | the member's identity id | `from` and `to`: the role keys |
| `member.removed`, `member.left` | the member's identity id | `role`: the role they held |
| `invitation.created` | the invitation's id | `role` and `label` |
| `invitation.revoked` | the invitation's id | nothing |
| `invitation.accepted` | the invitation's id | `role`; the actor is the person who joined |
| `token.created`, `token.revoked` | the token's id | its `name`, `kind`, `permissions` and `expires_at` |

### `GET /api/v1/projects/{project}/audit`

The project's events, newest first.

- **Who:** `audit.read` (owners), checked against your role as it is now. Not an API token.
- **Query:**

| Parameter | Meaning |
|---|---|
| `limit` | Events per page, 1 to 100, default 50. |
| `cursor` | The `next_cursor` of the previous page; it works only for this project. |
| `action` | Only events with exactly this action, such as `token.created`. |
| `actor` | Only events by this identity: a person's id, or a token's `identity_id`. |

**Response:** `200`:

```json
{
  "data": [
    {
      "id": "0193f7a2-0b1c-7d2e-8f3a-4b5c6d7e8f90",
      "at": "2026-10-28T09:12:40.512841Z",
      "actor": { "id": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d", "name": "Ada Owner", "kind": "person" },
      "action": "member.role_changed",
      "subject": "0193f69e-0a1b-7c2d-8e3f-405162738495",
      "detail": { "from": "editor", "to": "copy-desk" }
    }
  ],
  "next_cursor": "YXVkaXQ6djE6…"
}
```

`actor.kind` is `person` or `token`; `actor.name` is a display name, or `null` when there is none.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `limit` outside 1 to 100, a cursor from another project, or an `actor` that isn't an id. |
| `403` | `forbidden` | `missing_permission` | Your role lacks `audit.read`. |
| `403` | `forbidden` | `api_token_not_allowed` | The caller is an API token. |
| `404` | `not_found` | | No such project, or you aren't a member. |

## Proof

`crates/colophon-server/tests/tokens.rs` against the real binary: `a_token_does_exactly_what_its_permissions_allow_and_stops_when_revoked` (a read-only token refused a write with `missing_permission`, a custom token's entries naming the token, another project `404`, people's routes `api_token_not_allowed`, `not_for_tokens`, `invalid_expiry`, revocation answered `401` from the next request) and `every_change_to_access_is_in_the_audit_log_which_only_owners_read` (every kind of change recorded in order, no secret in the log, pages and filters). `crates/colophon-db/tests/tokens.rs` against PostgreSQL: the log refuses the owner and the application alike, a revocation waits for a running request, and a token is invisible outside its project. The testbed's API tokens page, and `perf/k6/tokens.js`, run as a smoke so far.

## Not built yet

- A Studio page for tokens and the audit log. <Status planned="not scheduled" />
- Retention of the audit log, and its purge. <Status planned="P6" />
- Machine credentials for agents with budgets and quotas beyond what a token holds. <Status planned="P5" />
- Rate limits per token. <Status planned="S" />

## Design

D-063 is in `docs/DECISIONS.md` on the branch that builds R.4, with the token and audit sections of modules 02, 03 and 04; the [design documents](/design/) here show them once it is merged.
