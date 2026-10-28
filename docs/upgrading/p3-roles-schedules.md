# Upgrading to 3.6, 3.7, R.1 to R.3 and scheduled publishing: migrations 0016 to 0022

Applies to a database at migration 0015 (the end of [p3.md](p3.md)) or partway through these, moving to the commit
that merged P3 steps 3.6 and 3.7, milestone R's steps R.1 to R.3 and scheduled publishing on 2026-10-27 (merge
commits `6de850c`, `85aa1f1`, `c89c102`, `bf0ecd7`, `9991ec2` and `76cddf8`). P3 and R aren't finished: step 3.8 and
step R.4 are still to come. Time: not measured; 0016 backfills nothing, 0017 inserts four roles and their permissions
for every project, and 0022 one permission for every project's owner role. Downtime: yes, the server and the worker
are stopped while the migrations run.

Colophon is pre-release, with no versioned releases or binaries: upgrading means building from a later commit and
applying its migrations.

## Before you start

- [ ] Finish [p3.md](p3.md) first if `_sqlx_migrations` stops before 15.
- [ ] Note the commit you run now (`git rev-parse --short HEAD`); every crate is version 0.0.0.
- [ ] Take a copy of the database you could restore. There is no supported backup yet (P6).
- [ ] If you ran a branch of one of these steps against a database before they were merged, see *Known issues*:
      its migrations may differ from the merged ones byte for byte, and sqlx refuses that.

## Are you affected?

As the migrator:

```sql
select version, description, success from _sqlx_migrations where version >= 15 order by version;
```

| What you see | What it means |
|---|---|
| only `15 draft titles` | This guide applies in full. |
| `16 slugs` up to `21 narrowed permissions` | Partway: the migrations that are missing still run, in order. |
| `22 owners may delete` | Already up to date. |
| `15 slugs` | Migrated by the first commit of the 3.6 branch (`10f3c65`), when slugs were 0015. Nothing can migrate it now: drop it and start again. |

## What changes for you

- **Editors:**
  - an article can have a **URL name**: the part of a web address that names it. It is 1 to 120 lowercase letters and
    digits joined by single hyphens, refused rather than rewritten when it isn't one, and no two articles in a
    collection share one, among drafts or among what is published (3.6, D-054);
  - the Studio's content-type builder checks each change against stored content: removing a field, making one
    required or narrowing a limit is refused, with the first 20 entries in the way, while any draft or published
    version wouldn't fit (3.6, D-055);
  - owners can create roles of their own, choosing what each may do, and limit a permission to some fields or to a
    member's own entries (R.2, R.3). Fields your role can't see are left out of the editor; fields you can see but not
    change are shown disabled. R.1 alone changes nothing you can see;
  - an approved publish request can be scheduled for a date, time and time zone, and cancelled until then (D-060);
  - the Studio uses plain words on every screen (publish request, version, URL name, website connections), offers
    *Publish* or *Ask for review* on an entry, and shows a first-run checklist on a project's overview (3.7, D-059).
- **Developers:**
  - a `slug` field type; a URL name that is taken answers `409` with `details.reason` `slug_taken` and the entry in
    the way; a content-type change that stored content doesn't fit answers `422` with `needs_migration` (3.6);
  - `GET`, `POST`, `PUT` and `DELETE` on `/api/v1/projects/{project}/roles` and `/roles/{role}` (R.2). A role has a
    version: changes and deletes need `If-Match`, a stale one answers `409` `stale_role`, and deleting a role someone
    holds answers `409` `role_in_use`. A permission is an action and a subject, a collection or `*`, and since R.3
    optionally `fields` and `"condition": "own"`;
  - entries leave out fields the caller's role can't read (`hidden_fields`) and list those it can't change
    (`locked_fields`); a save that changes a field the role can't change answers `422` `field_not_permitted`; an entry
    the role can't read is `404` (R.3);
  - `POST {P}/changesets/{changeset}/schedule`, `GET {P}/schedules` and `DELETE {P}/schedules/{schedule}`; a local
    time the clocks show twice or skip answers `422` with both candidate instants in `details.candidates` (D-060);
  - `POST /api/v1/setup`, `/auth/login?start=create`, `installation.has_administrator` on `/me`, and `self_review` on
    `GET /api/v1/projects/{project}` (3.7, D-061); a new error code, `rate_limited` (`429` with `Retry-After`).
- **Operators:**
  - while nobody is an administrator, `colophon-server` prints a one-time setup code to standard error when it starts,
    valid for an hour; the first person to enter it in the Studio becomes the administrator. Run
    `colophon admin bootstrap` before the first start if you want no code ever issued (D-061);
  - the worker also runs scheduled publishes, in a loop beside the deliveries, every `COLOPHON_WORKER_POLL_MS`;
  - the local Keycloak realm allows self-registration and accepts callbacks on ports 58400 to 58405: recreate the
    container to load it (`docker compose -f deploy/compose.yaml up -d --force-recreate keycloak`);
  - no new settings.

## Steps

1. Stop the worker if it runs on its own, then every other `colophon-server` process that uses the database.
2. Build the server, the CLI and the Studio from the commit you are moving to:

   ```sh
   cargo build --release --locked -p colophon-server -p colophon-cli
   npm ci && npm run build -w @colophon/studio
   ```

3. Apply the migrations as the database's owner. On the local stack:

   ```sh
   scripts/migrate.sh
   ```

   - **0016 slugs:** a checked `slug` column on revisions, `draft_slug` on entries, `slug` on publications, and two
     partial unique indexes (one among drafts, one among open publications). Every existing slug is empty, so nothing
     is backfilled.
   - **0017 roles:** the `roles` and `role_permissions` tables, the `colophon_seed_roles` function, the four built-in
     roles seeded into every existing project, and memberships and invitations made to reference a role of their
     project. Row-level security on `projects` is lifted for the seeding and forced again in the same transaction.
   - **0018 custom roles:** a `version` and `updated_at` on roles, triggers that refuse any change to the owner role or
     its permissions, invitations no longer tied to a role by foreign key, and the application allowed to change and
     delete roles.
   - **0019 schedules:** the `schedules` table, with partial indexes for pending and recently finished schedules and
     one pending schedule per publish request.
   - **0020 setup codes:** `setup_codes` and `setup_attempts`, which the application role can't read or write, and the
     four `SECURITY DEFINER` functions that are its only way in.
   - **0021 narrowed permissions:** one permission row per action, subject and condition, and checks that a field list
     names field ids and applies only to `content.read`, `content.create` and `content.update`, and `own` only to
     `content.read`, `content.update` and `content.delete`.
   - **0022 owners may delete:** `content.delete` for every owner role, new and existing. No route deletes entries
     yet.

4. Start the server (and the worker, if it runs on its own), then wait until `colophon doctor` exits 0.

## Check it worked

- `_sqlx_migrations` shows 16 to 22, each `success`.
- Every project has its four built-in roles:

  ```sql
  select p.id from projects p
  where (select count(*) from roles r where r.project_id = p.id and r.built_in) <> 4;
  ```

  returns no rows.
- The owner role refuses changes, even from the table owner:

  ```sql
  begin;
  update roles set name = name where key = 'owner';
  rollback;
  ```

  fails with `the owner role is fixed: it always holds every permission`.
- Every owner role holds `content.delete`:

  ```sql
  select r.project_id from roles r
  where r.key = 'owner' and not exists (
    select 1 from role_permissions p
    where p.project_id = r.project_id and p.role_key = 'owner' and p.action = 'content.delete');
  ```

  returns no rows.
- In the Studio, a project's settings list its roles, each with a page showing what it may do and who holds it, and
  an owner can create and edit roles there (R.2).
- In a content type with a URL-name field, saving a second article with a URL name the first already has is refused,
  naming the first.
- An approved publish request scheduled a minute ahead is published by the worker as the next release, and the
  Releases page shows it as published on time.

## Rolling back

- **An older build refuses the database.** Its `colophon admin migrate` stops with `migration 16 was previously
  applied but is missing in the resolved migrations` (or the first number it doesn't know), and its server may fail
  on the changed constraints. There are no down migrations: go back to the copy you kept at 0015.

## Known issues

- **Databases migrated by the branches.** A database migrated by an earlier commit of one of these steps' branches
  stops with `migration N was previously applied but has been modified` when a merged migration differs from the one
  it ran; sqlx checks every byte, comments included. It happened three times: slugs were `0015_slugs.sql` in `10f3c65` before
  `main` took 0015 for draft titles; `0018_custom_roles.sql` changed (its comment) in `5cc99e4` after `ff88120`; and
  `0019_schedules.sql` changed (its opening comment) in `db542c4`, on R.3's branch, which merged after scheduling, so
  a database migrated by the `e/schedules` branch or at the schedules merge `bf0ecd7` is refused too. Drop such a
  database and migrate again.
- **Invitations and deleted roles.** Since 0018 an invitation's role isn't a foreign key; inviting holds a shared
  advisory lock on the role, and deleting a role takes it exclusively and refuses while a pending invitation offers it.
- **A setup code can sit in collected logs** for its hour: container log collectors capture standard error. It dies
  at first use, and whenever anyone becomes an administrator (D-061).
- **API tokens and the audit log** are R.4; `tokens.manage` and `content.delete` are held by owners but no route uses
  them yet.

The migrations:
[0016](../../crates/colophon-db/migrations/0016_slugs.sql),
[0017](../../crates/colophon-db/migrations/0017_roles.sql),
[0018](../../crates/colophon-db/migrations/0018_custom_roles.sql),
[0019](../../crates/colophon-db/migrations/0019_schedules.sql),
[0020](../../crates/colophon-db/migrations/0020_setup_codes.sql),
[0021](../../crates/colophon-db/migrations/0021_narrowed_permissions.sql),
[0022](../../crates/colophon-db/migrations/0022_owners_may_delete.sql).
Decisions D-054, D-055 and D-058 to D-061 are in [DECISIONS.md](../DECISIONS.md).
