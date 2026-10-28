<script>
	import Status from '#lib/components/Status.svelte';
</script>

A role belongs to a project and is a list of permissions. Every project starts with four built-in roles; people whose role has `roles.manage` (owners, by default) define more, change them and delete them. Milestone R built them in steps (D-058): R.1 stored the four roles as data with unchanged powers (migration 0017), R.2 added these routes and custom roles (migration 0018), and R.3 lets a permission be narrowed to some fields or to its holder's own entries (migration 0021). Handler: `crates/colophon-api/src/roles.rs`; evaluator: `crates/colophon-domain/src/policy.rs`. The concepts are in [Roles and permissions](/concepts/roles-and-permissions/).

## Permissions

A permission is an action and what it applies to, optionally narrowed to some fields or to the entries its holder created:

```json
{ "action": "content.update", "subject": "pages" }
{ "action": "content.update", "subject": "articles", "fields": ["title", "body"], "condition": "own" }
```

| Action | Allows | Per collection |
|---|---|---|
| `content.read` | Reading entries and their versions. | yes |
| `content.create` | Creating entries. | yes |
| `content.update` | Saving entries, and creating, submitting and refreshing publish requests. | yes |
| `content.delete` | Deleting entries. No route offers it yet. | yes |
| `content.review` | Approving a publish request, or asking for changes. | yes |
| `content.publish` | Publishing an approved request. | yes |
| `schema.change` | Creating and changing content types. | yes |
| `members.manage` | Inviting people, changing their roles, removing them. | no |
| `roles.manage` | Creating, changing and deleting roles. | no |
| `destinations.manage` | Seeing and changing website connections. | no |
| `tokens.manage` | Creating and revoking API tokens. No route offers it yet. <Status planned="R.4" /> | no |

- `subject` is `*` (every collection, and the project itself) or one collection's slug. A slug that no content type has yet is allowed, so a role can be ready before its collection is.
- The last four actions belong to the whole project, so their `subject` must be `*`.
- **Deny by default.** A request is allowed only if a permission names its action on `*` or on that very collection. A publish request that spans several collections needs the action in every one of them. A narrowed permission counts only where an entry's fields or creator are asked about (below); it never grants the action collection-wide.
- **Nobody hands out more than they hold.** A role's powers cover another's when every permission of the other is allowed by the first. Creating, changing or deleting a role, giving someone a role or taking it away, and inviting to a role all need the caller's role, as it is at that moment, to cover the roles involved. So no chain of grants ends with more power than it started with (`nobody_hands_out_more_than_they_hold` in `crates/colophon-server/tests/permissions.rs`).
A changed role holds from its holders' next request; a request already running acts on the permissions it read when it began.

### Narrowing a permission

Since step R.3, two optional members narrow a permission:

| Member | On | Means |
|---|---|---|
| `fields` | `content.read`, `content.create`, `content.update` | Only these fields: 1 to 100 field ids (1 to 64 lowercase letters, digits and underscores, starting with a letter), sorted and without duplicates when stored. A field no content type has yet is allowed. |
| `condition` | `content.read`, `content.update`, `content.delete` | `"own"`, the one condition: only entries the member created. |

- **Decided per entry.** For an entry, every permission that applies (its action, its subject `*` or the entry's collection, its condition met) adds its fields, and one without `fields` allows them all. So "titles of any article" beside "every field of my own articles" lets its holder change every field of their own articles and the titles of the rest. What [entries](/reference/api/entries/#fields-and-own-entries) do with it: leave out fields a role can't read, refuse changes to fields it can't change (`field_not_permitted`), and answer `404` for an entry it can't read at all.
- **One permission per action, subject and condition.** Permissions that share all three are merged, with the union of their fields (or every field, if one has no list); a role may hold the same action twice, once for every entry and once for its own. Migration 0021 holds the same rule in the database.
- **Covering is exact.** Nobody hands out a field or an entry they can't reach themselves: giving `content.update` on `title`, or on one's own entries, needs that much or more.
- **The other actions can't be narrowed.** `content.review`, `content.publish`, `schema.change` and the four project-wide actions take neither member.

## Built-in roles

Seeded in every project with exactly D-042's powers (migration 0017; `the_built_in_roles_keep_d_042_s_powers` in `policy.rs`). All apply to `*`.

| Key | Name | `content.read`, `.create`, `.update` | `content.review`, `.publish` | `schema.change`, `destinations.manage` | `members.manage`, `roles.manage`, `tokens.manage` |
|---|---|---|---|---|---|
| `owner` | Owner | yes | yes | yes | yes |
| `developer` | Developer | yes | no | yes | no |
| `editor` | Editor | yes | no | no | no |
| `publisher` | Publisher | yes | yes | no | no |

Owners also hold `content.delete` (migration 0022), so that it can be given at all, since nobody hands out what they don't hold; no route deletes entries yet. The owner role is **fixed**: it always holds every permission, and can't be changed or deleted (`409`); migration 0018's triggers refuse it even for a direct database write. The other three are ordinary roles marked `built_in`: they can be changed, and deleted when nobody holds them.

## The role object

```json
{
  "key": "copy-desk",
  "name": "Copy desk",
  "description": "Edits pages; can't publish.",
  "built_in": false,
  "fixed": false,
  "version": 2,
  "permissions": [
    { "action": "content.read", "subject": "*" },
    { "action": "content.update", "subject": "articles", "fields": ["title"] },
    { "action": "content.update", "subject": "articles", "condition": "own" },
    { "action": "content.update", "subject": "pages" }
  ],
  "members": 3,
  "invitations": 1,
  "updated_at": "2026-10-27T10:02:44.903117Z"
}
```

| Field | Type | Meaning |
|---|---|---|
| `key` | string | The role's id in the project, as memberships and invitations name it. Never changes. |
| `name`, `description` | string | For people. |
| `built_in` | boolean | One of the four every project starts with. |
| `fixed` | boolean | The owner role. |
| `version` | integer | Starts at 1; every change adds one. Its `ETag` is `"v<version>"`. |
| `permissions` | array | Sorted. Each has `action` and `subject`, and `fields` and `condition` only when it is narrowed. |
| `members` | integer | Members who hold it. |
| `invitations` | integer | Pending invitations that offer it. |
| `updated_at` | string | When it last changed. |

## Routes

### `GET /api/v1/projects/{project}/roles`

The project's roles: what each one allows, before anyone is given it.

- **Who:** any member.

**Response:** `200`, built-in roles first, then in the order they were made, at most 100 (a project's maximum), `next_cursor` always `null`.

```json
{
  "data": [
    {
      "key": "editor",
      "name": "Editor",
      "description": "Writes and edits content.",
      "built_in": true,
      "fixed": false,
      "version": 1,
      "permissions": [
        { "action": "content.read", "subject": "*" },
        { "action": "content.create", "subject": "*" },
        { "action": "content.update", "subject": "*" }
      ],
      "members": 4,
      "invitations": 0,
      "updated_at": "2026-10-20T08:00:12.004417Z"
    }
  ],
  "next_cursor": null
}
```

The list holds every role; this example shows one.

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such project, or you aren't a member. |

### `POST /api/v1/projects/{project}/roles`

Creates a role.

- **Who:** `roles.manage` (owners), and your role must cover every permission you give the new one.
- **Headers:** no `Idempotency-Key`: a retry is refused as a taken key.
- **Body:**

```json
{
  "key": "copy-desk",
  "name": "Copy desk",
  "description": "Edits pages; can't publish.",
  "permissions": [
    { "action": "content.read", "subject": "*" },
    { "action": "content.update", "subject": "pages" }
  ]
}
```

| Field | Type | Rules |
|---|---|---|
| `key` | string | Required. 1 to 63 lowercase letters, digits and hyphens, starting with a letter. Unique in the project. |
| `name` | string | Required. Trimmed, 1 to 100 characters, no control characters. |
| `description` | string | Optional, default empty. Trimmed, at most 500 characters, no control characters (no line breaks). |
| `permissions` | array | Required, at most 500; may be empty. Each `{"action", "subject"}`, with optional `fields` and `condition` ([narrowing](#narrowing-a-permission)); permissions with the same action, subject and condition are merged. No other member is accepted. |

**Response:** `201 Created`, `ETag: "v1"`, `{"data": …}` with [the role object](#the-role-object).

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `403` | `forbidden` | `missing_permission`, `cannot_grant` | Your role lacks `roles.manage`, or a power you are giving. |
| `409` | `conflict` | `role_exists` | The project has a role with this key. |
| `422` | `validation_failed` | `invalid_role_key` | `/key` breaks the rule. |
| `422` | `validation_failed` | `invalid_role_name` | `/name` breaks the rule. |
| `422` | `validation_failed` | `invalid_role_description` | `/description` breaks the rule. |
| `422` | `validation_failed` | `too_many_permissions` | More than 500 in `/permissions`. |
| `422` | `validation_failed` | `unknown_action` | A permission's action isn't one of the eleven. The message names its position. |
| `422` | `validation_failed` | `invalid_subject` | A `subject` is neither `*` nor a collection slug. |
| `422` | `validation_failed` | `project_wide_action` | A project-wide action names one collection. |
| `422` | `validation_failed` | `fields_not_applicable` | `fields` on an action other than `content.read`, `content.create` or `content.update`. |
| `422` | `validation_failed` | `invalid_fields` | `fields` is empty, has more than 100 ids, or names something that isn't a field id. |
| `422` | `validation_failed` | `unknown_condition` | `condition` isn't `own`. |
| `422` | `validation_failed` | `condition_not_applicable` | `own` on an action other than `content.read`, `content.update` or `content.delete`. |
| `422` | `validation_failed` | `too_many_roles` | The project has 100 roles already. |

### `GET /api/v1/projects/{project}/roles/{role}`

One role.

- **Who:** any member.
- **Path:** `role`, its key.

**Response:** `200`, `ETag: "v<version>"`, `{"data": …}` with [the role object](#the-role-object).

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | No such role here. |

### `PUT /api/v1/projects/{project}/roles/{role}`

Replaces a role's name, description and permissions. Its key never changes.

- **Who:** `roles.manage` (owners), and your role must cover both its permissions now and the new ones.
- **Headers:** `If-Match: "v<version>"`, the version you edited (required).
- **Body:** as for creating, without `key` (sending one is `400` `malformed_json`):

```json
{ "name": "Copy desk", "description": "Edits pages and articles.", "permissions": [ { "action": "content.read", "subject": "*" }, { "action": "content.update", "subject": "*" } ] }
```

**Response:** `200`, `ETag: "v<new version>"`, the role as stored now.

Changing a role locks only that role; its version decides which of two concurrent edits wins.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` isn't one role `ETag`, such as `"v3"`. |
| `403` | `forbidden` | `missing_permission`, `cannot_grant` | Your role lacks `roles.manage`, or a power of the role before or after. |
| `404` | `not_found` | | No such role. |
| `409` | `conflict` | `owner_role_fixed` | The role is `owner`, which is fixed. |
| `409` | `conflict` | `stale_role` | It changed since your version; the `ETag` header names the current one. |
| `422` | `validation_failed` | as for creating | A field or permission breaks a rule. |
| `428` | `precondition_required` | | No `If-Match`. |

### `DELETE /api/v1/projects/{project}/roles/{role}`

Deletes a role nobody uses.

- **Who:** `roles.manage` (owners), and your role must cover the role's permissions.
- **Headers:** `If-Match: "v<version>"` (required).

**Response:** `204 No Content`.

A role that members hold, or that pending invitations offer, can't be deleted: give those members another role, or revoke the invitations, first. Inviting holds a lock on the role it names, so a role can't be deleted while an invitation to it is being made.

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `400` | `bad_request` | | `If-Match` is malformed. |
| `403` | `forbidden` | `missing_permission`, `cannot_grant` | Your role lacks `roles.manage`, or a power of this role. |
| `404` | `not_found` | | No such role. |
| `409` | `conflict` | `owner_role_fixed` | The role is `owner`. |
| `409` | `conflict` | `role_in_use` | Members hold it or pending invitations offer it; the message says how many of each. |
| `409` | `conflict` | `stale_role` | It changed since your version; the `ETag` header names the current one. |
| `428` | `precondition_required` | | No `If-Match`. |

## Proof

- `crates/colophon-domain/src/policy.rs`: the built-in table, collection scoping, `covers`, permission parsing, and fields and `own` decided per entry, as unit tests.
- `crates/colophon-db/tests/roles.rs`: roles against real PostgreSQL, including the fixed owner role, versions, and narrowed permissions stored one per action, subject and condition.
- `crates/colophon-server/tests/permissions.rs`: against the real binary, a role the owner defines decides what its holder may do, nobody hands out more than they hold, and field lists and `own` hold on reads, saves, unreadable entries (`404`) and publish requests (`an_author_reads_and_edits_titles_of_any_article_and_all_of_their_own`).
- `crates/colophon-api/src/visibility.rs`: what a read leaves out and what a save may change, as unit tests.
- `perf/k6/roles.js`: a load scenario with custom roles; its full runs are on [Performance](/resources/performance/).

## Not built yet

- API tokens with the same permissions, and an audit log of role, membership and token changes. <Status planned="R.4" />
- `media.*` actions with the media library, and `locales.*` with localization. <Status planned="P4, E" />
