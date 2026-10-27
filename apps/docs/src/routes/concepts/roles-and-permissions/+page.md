<script>
	import Status from '#lib/components/Status.svelte';
</script>

Who may do what is decided in one place, checked on every request, and backed by the database so a bug in a handler can't leak another project's data. Four fixed roles, invitations and the last-owner rule are done (step 1.7). Milestone R turns roles into data: step R.1 stores the four roles as rows with unchanged powers, and step R.2 lets owners define roles of their own. Both are on branches and not merged; R.2's branch, `r/r.2-custom-roles`, contains R.1. Field-level permissions, "only their own" conditions, API tokens and an audit log are planned for R.3 and R.4.

## Projects are isolated by the database <Status done="1.1" />

Every table that belongs to a project carries its id, and PostgreSQL's row-level security hides other projects' rows from the server's database role (`colophon_api`, which the login `colophon_app` uses), which can't bypass it. A project you aren't a member of answers `404`, exactly like one that doesn't exist. Roles decide what you may do *inside* a project; they never widen what the database lets you see. The [security model](/resources/security/) has the database roles and the tests.

## Actions and the built-in roles

A permission names an **action** and a **subject**: `*` (every collection, and the project itself) or one collection. Seven actions are about content and may name one collection; four are about the whole project and only ever apply to `*`. This is `colophon_domain::policy` on the R.2 branch, and the built-in roles' permissions are `Policy::builtin`:

| Action | What it allows | Owner | Developer | Editor | Publisher |
|---|---|---|---|---|---|
| `content.read` | read entries and their versions | yes | yes | yes | yes |
| `content.create` | create entries | yes | yes | yes | yes |
| `content.update` | save versions; make, submit and refresh publish requests | yes | yes | yes | yes |
| `content.delete` | delete entries (no route offers it yet) | no | no | no | no |
| `content.review` | approve a publish request, or ask for changes | yes | no | no | yes |
| `content.publish` | publish an approved request as a release | yes | no | no | yes |
| `schema.change` | create and change content types | yes | yes | no | no |
| `members.manage` | invite people, change their roles, remove them | yes | no | no | no |
| `roles.manage` | create, change and delete roles | yes | no | no | no |
| `destinations.manage` | manage website connections (webhooks, CDNs, probes) | yes | yes | no | no |
| `tokens.manage` | create and revoke API tokens (none exist yet) | yes | no | no | no |

The first seven rows (content and `schema.change`) may name one collection. These are exactly the powers of step 1.7's fixed table (D-042), plus `roles.manage` and `tokens.manage` for owners. On `main` the same powers come from a fixed table over the four roles. <Status done="1.7" />

Any member lists the members and the roles, and may leave. Anyone can see what a role allows before it is given to them.

## Permissions are data <Status building="R.1" />

Decision D-058 replaces the fixed table with rows. Migration 0017 adds `roles` and `role_permissions` (an action, a subject, and a field list and condition stored for R.3), seeds the four roles above into every project through `colophon_seed_roles`, and makes each membership's and invitation's role a foreign key to one of its project's roles. The key stays the readable name (`editor`), so URLs, logs and the API don't change.

- **One evaluator.** `Policy::allows(action, collection)` has no I/O: deny unless a permission for that action names `*` or that collection. A check that must hold everywhere passes no collection and holds only for `*`.
- **Every handler asks it.** Entries check `content.read`, `content.create` or `content.update` for their own collection. A publish request checks `content.update` for every collection it pins, its review `content.review`, and publishing `content.publish` for each collection, after locking them. Content types check `schema.change` for theirs; website connections `destinations.manage`; members and invitations `members.manage`.
- **Loaded with the membership.** `scope::begin_as_member` reads the caller's membership and their role's permissions in one statement, so every project transaction carries its policy. A stored action or condition the code doesn't know is an error (`DbError::Corrupt`), never a skipped permission.
- **Behaviour unchanged.** A test holds `colophon_seed_roles` equal to `Policy::builtin`, the D-042 table test still passes, and the server suites pass unchanged.
- **Row-level security** covers both new tables. Members can read their own project's roles before its context is set, because the membership lookup reads the permissions in the same statement.

## Roles owners define <Status building="R.2" />

Since R.2, `/api/v1/projects/{project}/roles` lists and reads a project's roles (any member), and creates, changes and deletes them (people whose role has `roles.manage`; owners by default). The [Roles reference](/reference/api/roles/) has the routes.

- A role has a key (`copy-desk`: 1 to 63 lowercase letters, digits and hyphens, starting with a letter), a name, a description and its permissions, each `{"action": "content.update", "subject": "pages"}`. A project has at most 100 roles, the built-in ones included, and a role at most 500 permissions.
- **Versions.** A role has a version; reading one returns `ETag: "v3"`. Changing or deleting one needs `If-Match` with it, so two people editing one role can't overwrite each other: a stale version is `409 stale_role` with the current `ETag`.
- **The owner role is fixed.** The API answers `409`, and migration 0018's triggers refuse any update or delete of its row or its permissions, even by a direct write as the application's login. So no path can leave a project whose owners can't manage it.
- **A role in use can't be deleted**: `409 role_in_use`, saying how many members hold it and how many pending invitations offer it.
- **Field lists and conditions are refused** with `422 not_yet_enforced` until R.3 enforces them. Stored but unenforced, they would look like restrictions that aren't there.
- **Changes hold from the next request.** A member whose role changed gets its new powers on their next request; a request already running acts on the permissions it read when its transaction began.

In the Studio, Settings lists every role with what it's for and how many people hold it. A role's page shows what it may do as a grid in plain words: *See*, *Create*, *Edit*, *Approve*, *Publish* and *Change fields*, for *Everything* (every content type, including ones added later) and for each content type, then the powers over the whole project. People whose role manages roles edit it; everyone else reads it. The Studio decides which controls to offer with one `may(action, collection)` check that mirrors `Policy::allows`, instead of comparing role names, and `GET /api/v1/projects/{project}` returns the caller's role and permissions for it. The server still decides every request.

## Nobody hands out more than they hold <Status building="R.2" />

The rule behind R.2 is `Policy::covers`: a holder of one policy may hand out another only if their own policy allows every permission in it. A permission on `*` needs the action on `*`; a permission on one collection needs the action on `*` or on that collection. So no chain of grants ends with more power than it started with.

| To | You need |
|---|---|
| create a role | `roles.manage`, and every power of the new role |
| change a role | `roles.manage`, and every power of the role before and after the change |
| delete a role | `roles.manage`, and every power of the role |
| change someone's role | `members.manage`, and every power of their current role and of the new one |
| remove someone | `members.manage`, and every power of their role (anyone may leave) |
| invite someone | `members.manage`, and every power of the role offered |

Each check uses the caller's role **as it is now**, read again inside the transaction, not the role read when the request began, so a demotion that commits at the same moment grants nothing. Member changes are re-checked under the owner lock below; an invitation reads the caller's role and the role offered in one locking statement, and holds a shared advisory lock on that role so it can't be deleted before the invitation commits. Owners skip the second read: the owner role holds every permission any role can hold, and migration 0018 keeps it so.

So a role that manages members but can't change content types can't demote a developer, promote itself to owner, or remove an owner; it can give its own role. An empty role is anyone's to give. Because no role holds `content.delete`, no role can be given it either.

Refusals are `403 forbidden` with a message that says why; an unknown role is `422 unknown_role`.

## The last owner <Status done="1.7" />

A project always keeps an owner. A member change locks the project's owners, the caller and the target in one statement, in identity order, so changes to one project's members queue and can't deadlock. If the change would leave no owner, it is refused with `409`; and the deferred constraint trigger `memberships_keep_an_owner` (migration 0005) refuses any commit that would, whatever code path made it.

## Invitations <Status done="1.7" />

A person with `members.manage` invites by creating a link, `/studio/invite#<token>`, that grants one role in one project, works once, and expires after 72 hours. The token is 256 random bits and travels after the `#`, which browsers never send to a server, so it can't end up in access logs or a `Referer` header; only its SHA-256 hash is stored, and lookup and acceptance take it in a JSON body. Before accepting, the holder sees the project's name, the role and who invited them; on the R.2 branch also the role's name and description. Accepting binds the membership to whoever is signed in; someone already in the project gets `409` and the invitation stays unused.

Invitations aren't tied to an email address, because an email address never establishes identity here: whoever holds the link can use it, and it can be revoked at any time. On the R.2 branch an invitation can offer any of the project's roles, by key. <Status building="R.2" />

## The first administrator <Status done="1.8" />

Only administrators create projects, and nobody becomes one by signing in. The first is made once, by the operator, from a terminal, with the database owner's credentials: `colophon admin bootstrap --subject S`. On `main`, no HTTP route can create an administrator. Step 3.7's branch adds a one-time setup code that only the server's terminal shows, entered in the browser while nobody is an administrator yet. <Status building="3.7" /> Details are in the [security model](/resources/security/).

## How it is tested

- `crates/colophon-domain/src/policy.rs`: the built-in roles keep D-042's powers in every collection and collection-wide, a collection permission allows that collection only, nobody hands out power they don't hold, and permissions parse as stored or not at all. <Status building="R.2" />
- `crates/colophon-db/tests/roles.rs`: a new project's seeded roles equal `Policy::builtin`; a role is created, changed only at its version, holds from its holder's next transaction, and is deleted only once unused; the owner role refuses a direct write; a member manager can't demote a developer, promote himself or remove an owner. <Status building="R.2" />
- `crates/colophon-server/tests/permissions.rs`, against the real binary with two people signed in: a role the owner defines decides what its holder may do from their next request, `stale_role`, `role_in_use`, the fixed owner role, refused fields and conditions, and escalation refused over HTTP. <Status building="R.2" />
- `apps/studio/tests/server/permissions.spec.ts` and `apps/testbed/tests/roles.spec.ts`: make a role, tick what it may do, lose a stale save in a second tab, invite someone who holds exactly it, a held role that can't be deleted, with axe on every Studio page. <Status building="R.2" />
- `crates/colophon-db/tests/members.rs` and `crates/colophon-server/tests/members.rs`: an invitation joins once; revoked and expired invitations are dead; the last owner can be neither demoted nor removed; two owners demoting each other at the same moment leave one owner; an owner demoted while their request runs can neither promote nor remove anyone. <Status done="1.7" />

## Load

`perf/k6/roles.js` is R.2's load scenario: half the requests are a member holding a custom role listing articles, one in ten reads the project with their permissions, and the rest are an owner listing every role and editing roles with `If-Match`. `scripts/load.sh` runs it at 1,000 requests a second with a 100 ms p99 threshold; CI runs a smaller smoke. It exists only on the R.2 branch:

```sh
scripts/load.sh roles throughput
```

The numbers so far are smoke runs, not a full million-request run, which the branch says waits for a quiet machine. On the maintainer's laptop (Apple M2 Pro, 12 cores, 32 GB) on 27 October 2026, while it was also building a site: p99 72 ms for role edits, 63 ms for the custom role's entry lists, 58 ms for role lists, and no failed requests. In `perf/k6/members.js` on the same laptop, at 1,000 requests a second with fresh databases, a role change takes 4.2 ms uncontended (4.0 ms on `main`), and under load the medians were 9 and 23 ms for role changes and 8 and 15 ms for invitations in two runs, against 27 to 47 and 7 to 30 ms on `main`. These are measurements of one laptop on one day, not production capacity.

## Not built yet

- **Field-level permissions and `own` conditions.** A field list narrows what a write may change (a change to any other field refused with `field_not_permitted` and its pointer) and what a read returns (other fields left out, and the response says so); `own` limits a permission to entries the caller created. List and delivery projections that differ by caller will key caches on the permission set. <Status planned="R.3" />
- **API tokens** (read-only, full access, or a custom permission set) on hashed, expiring credentials, and an **append-only audit log** of role, membership and token changes. <Status planned="R.4" />
- Random roles checked against the evaluator in the simulator, and a property test for random roles. <Status planned="R" />
- Asking for a fresh sign-in before sensitive changes such as role changes. <Status planned="not scheduled" />
- Scoped, budgeted credentials for scripts and agents, and a kill switch. <Status planned="P5" />
- More than one administrator, through a grant flow. <Status planned="not scheduled" />

## Design

[Module 03, auth](/design/modules/03-auth/) and decisions D-042 and D-043 in the [decisions](/design/decisions/) describe what is on `main`. Milestone R is on its branch: D-058 in [DECISIONS.md](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/DECISIONS.md), and [module 03](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/modules/03-auth.md) and [module 02](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/modules/02-db.md) as R.2 left them. The milestone's steps are in the [roadmap](/resources/roadmap/).
