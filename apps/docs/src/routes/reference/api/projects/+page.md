<script>
	import Status from '#lib/components/Status.svelte';
</script>

A project is the unit of isolation: its own members, roles, content types, entries and releases, kept apart by the database's row-level security. This page covers creating and reading projects, the member list, and single-use invitation links. Handlers: `crates/colophon-api/src/projects.rs` and `members.rs`. The reasoning is in [Roles and permissions](/concepts/roles-and-permissions/); the editor's side is [Joining a project](/editors/joining/).

Since R.2, a member's role is any of the project's roles, by its key, and nobody gives, takes away or offers a role with a power their own role lacks <Status building="R.2" />. On `main`, a role is one of the four built-in ones and only owners manage members; for those four roles, the answers are the same.

## Projects

### `GET /api/v1/projects`

The projects you belong to.

- **Who:** signed in.

**Response:** `200`, by name, at most 1,000 on one page (`next_cursor` is always `null`).

```json
{
  "data": [
    { "id": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f", "slug": "field-notes", "name": "Field Notes", "role": "owner", "role_name": "Owner" }
  ],
  "next_cursor": null
}
```

| Field | Type | Meaning |
|---|---|---|
| `id` | UUID | The project's id: `{project}` in every other route. |
| `slug` | string | Its unique, URL-safe name. |
| `name` | string | Its display name. |
| `role` | string | Your role's key. |
| `role_name` | string | Your role's name <Status building="R.2" />. |

### `POST /api/v1/projects`

Creates a project, with a `production` environment, the built-in `articles` content type and the four built-in roles, and makes you its owner, in one transaction.

- **Who:** administrators only (D-043). The first administrator is made by the operator from a terminal, `colophon admin bootstrap` ([Command line](/reference/cli/)); no HTTP route can make one.
- **Headers:** no `Idempotency-Key`: a retry is refused as a taken slug.
- **Body:**

```json
{ "slug": "field-notes", "name": "Field Notes" }
```

| Field | Type | Rules |
|---|---|---|
| `slug` | string | Required. 1 to 63 lowercase letters, digits and hyphens, not starting or ending with a hyphen. Unique on the server. |
| `name` | string | Required. Trimmed, 1 to 200 characters, no control characters. |

**Response:** `201 Created`, `{"data": …}` with the project as listed above.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | | You aren't an administrator, or the installation has none yet. The message says which. |
| `409` | `conflict` | | A project with this slug exists. |
| `422` | `validation_failed` | `invalid_slug` | `/slug` breaks the rule. |
| `422` | `validation_failed` | `invalid_name` | `/name` breaks the rule. |

### `GET /api/v1/projects/{project}`

One project, with what your role may do in it, so a client can offer only what will work. The server still checks every request.

- **Who:** any member.

**Response:** `200`.

```json
{
  "data": {
    "id": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f",
    "slug": "field-notes",
    "name": "Field Notes",
    "role": "copy-desk",
    "role_name": "Copy desk",
    "permissions": [
      { "action": "content.read", "subject": "*" },
      { "action": "content.update", "subject": "pages" }
    ]
  }
}
```

`permissions` <Status building="R.2" /> lists your role's permissions, sorted, in the [roles' shape](/reference/api/roles/#permissions). It appears only here, not in lists or `/me`.

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such project, you aren't a member, or the id isn't a UUID. |

## Members

### `GET /api/v1/projects/{project}/members`

Who is in the project.

- **Who:** any member.

**Response:** `200`, owners first, then by name, at most 1,000 on one page.

```json
{
  "data": [
    { "identity_id": "0193f69e-0a1b-7c2d-8e3f-405162738495", "display_name": "Ada Owner", "role": "owner", "joined_at": "2026-10-20T08:00:12.004417Z" },
    { "identity_id": "0193f6c4-9e0f-7a1b-8c2d-3e4f5a6b7c8d", "display_name": "Sam Writer", "role": "editor", "joined_at": "2026-10-21T13:42:51.770193Z" }
  ],
  "next_cursor": null
}
```

### `PUT /api/v1/projects/{project}/members/{identity}`

Gives a member another role.

- **Who:** `members.manage` (owners). Since R.2 your role, as it is at that moment, must also hold every power of both the member's current role and the new one, so a role that manages members can't promote itself to owner or demote a developer <Status building="R.2" />.
- **Path:** `identity`, the member's `identity_id`.
- **Body:**

```json
{ "role": "publisher" }
```

| Field | Type | Rules |
|---|---|---|
| `role` | string | Required. One of the project's role keys: 1 to 63 lowercase letters, digits and hyphens, starting with a letter. |

**Response:** `204 No Content`. The member's next request acts with the new role.

Owner rows are locked before the change and the caller's role is read again under that lock, so concurrent changes queue and a role read a moment before a demotion grants nothing. A project always keeps an owner.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | | Your role lacks `members.manage`, or (R.2) one of the two roles' powers. |
| `404` | `not_found` | | No such member. |
| `409` | `conflict` | | The project would lose its last owner. |
| `422` | `validation_failed` | `invalid_role` | `/role` isn't shaped like a role key. |
| `422` | `validation_failed` | `unknown_role` | The project has no role with that key <Status building="R.2" />. |

### `DELETE /api/v1/projects/{project}/members/{identity}`

Removes a member, or lets you leave.

- **Who:** anyone, to remove themselves. To remove someone else: `members.manage` (owners), and since R.2 every power of the role they hold <Status building="R.2" />.

**Response:** `204 No Content`.

| Status | `code` | When |
|---|---|---|
| `403` | `forbidden` | Removing someone else without the permission or (R.2) their role's powers. |
| `404` | `not_found` | No such member. |
| `409` | `conflict` | The project would lose its last owner, you included. |

## Invitations

An invitation is a link, `/studio/invite#` followed by a token, that grants one role in one project, works once, and expires after 72 hours. The token travels after the `#`, which browsers never send to a server, and the lookup and accept routes take it in the body, so it never appears in a request line or a log; it is stored only as a SHA-256 hash. It isn't tied to an email address: whoever holds the link can use it, and it can be revoked at any time.

### `GET /api/v1/projects/{project}/invitations`

The pending invitations: not accepted, revoked or expired.

- **Who:** `members.manage` (owners).

**Response:** `200`, newest first, at most 1,000. The link isn't shown again.

```json
{
  "data": [
    { "id": "0193f6d1-3a4b-7c5d-8e6f-7a8b9c0d1e2f", "role": "editor", "label": "Sam, copy editor", "created_at": "2026-10-27T09:00:00.120044Z", "expires_at": "2026-10-30T09:00:00.120044Z" }
  ],
  "next_cursor": null
}
```

### `POST /api/v1/projects/{project}/invitations`

Makes an invitation link.

- **Who:** `members.manage` (owners), read as your role is at that moment; since R.2, only to a role whose every power your own role has <Status building="R.2" />.
- **Headers:** no `Idempotency-Key`: a retry makes a second link, and the first can be revoked.
- **Body:**

```json
{ "role": "editor", "label": "Sam, copy editor" }
```

| Field | Type | Rules |
|---|---|---|
| `role` | string | Required. One of the project's role keys. |
| `label` | string | Required. Who it is for, as the inviter sees it. Trimmed, 1 to 100 characters, no control characters. |

**Response:** `201 Created`. `link` appears in this answer only.

```json
{
  "data": {
    "id": "0193f6d1-3a4b-7c5d-8e6f-7a8b9c0d1e2f",
    "role": "editor",
    "label": "Sam, copy editor",
    "created_at": "2026-10-27T09:00:00.120044Z",
    "expires_at": "2026-10-30T09:00:00.120044Z",
    "link": "http://localhost:58000/studio/invite#q7Rk2…43 characters…"
  }
}
```

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | | Your role lacks `members.manage`, or (R.2) a power of the role offered. |
| `422` | `validation_failed` | `invalid_role` | `/role` isn't shaped like a role key. |
| `422` | `validation_failed` | `unknown_role` | The project has no such role <Status building="R.2" />. |
| `422` | `validation_failed` | `invalid_label` | `/label` breaks the rule. |

### `DELETE /api/v1/projects/{project}/invitations/{invitation}`

Revokes an invitation that hasn't been used.

- **Who:** `members.manage` (owners), read as your role is at that moment.

**Response:** `204 No Content`.

| Status | `code` | When |
|---|---|---|
| `403` | `forbidden` | Your role lacks `members.manage`. |
| `404` | `not_found` | No such invitation, or it was already accepted or revoked. |

### `POST /api/v1/invitations/lookup`

What an invitation offers, for the person holding the link, before they accept.

- **Who:** signed in. Membership isn't needed: holding the token is the point.
- **Body:**

```json
{ "token": "q7Rk2…43 characters…" }
```

**Response:** `200`.

```json
{
  "data": {
    "project_id": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f",
    "project_name": "Field Notes",
    "role": "editor",
    "role_name": "Editor",
    "role_description": "Writes and edits content.",
    "invited_by": "Ada Owner",
    "expires_at": "2026-10-30T09:00:00.120044Z"
  }
}
```

`role_name` and `role_description` <Status building="R.2" /> let the page say what the role means before anyone joins.

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | Unknown, expired, revoked or used; or the token isn't 43 characters. |

### `POST /api/v1/invitations/accept`

Joins the project with the invitation's role, bound to whoever is signed in.

- **Who:** signed in.
- **Body:** `{"token": "…"}`, as for the lookup.

**Response:** `200`.

```json
{ "data": { "project_id": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f", "role": "editor" } }
```

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | Unknown, expired, revoked or already used. |
| `409` | `conflict` | You are already a member. The invitation stays unused, for whoever it was meant for. |

## Not built yet

- Field-level permissions and "only entries I created" conditions on roles. <Status planned="R.3" />
- An audit log of role and membership changes. <Status planned="R.4" />
- More than one administrator, through a grant flow. <Status planned="not scheduled" />
