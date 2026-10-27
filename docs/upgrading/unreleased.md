# Upgrading to unreleased work: migrations 0016 to 0018

> **Not merged.** These migrations exist only on step branches that are still being built. They may be renumbered or
> edited before they reach `main`, and `main`'s binaries refuse a database that has them. Apply them only to a
> database of its own that you can throw away.

Applies to a database at migration 0015 (today's `main`, see [p3.md](p3.md)) that you want to run a branch against,
as of 2026-10-27:

| Branch | Step | Migrations | Head |
|---|---|---|---|
| `p3/3.6-modelling` | 3.6 content types and URL names | 0016 | `ef5939c` |
| `r/r.1-roles-as-data` | R.1 roles as data (contains 3.6) | 0016, 0017 | `9b4b972` |
| `r/r.2-custom-roles` | R.2 custom roles (contains R.1) | 0016 to 0018 | `5cc99e4` |

Two more branches, `p3/3.7-onboarding` and `e/schedules`, have no migrations committed yet. Time: not measured; 0016
backfills nothing, 0017 inserts four roles and their permissions for every project. Downtime: yes, as for every
upgrade.

## Before you start

- [ ] **Use a separate database.** On the local stack, create one and point the scripts at it with
      `COLOPHON_TEST_DATABASE`, so the database other checkouts share never gets these migrations:

      ```sh
      docker compose -f deploy/compose.yaml exec -T postgres psql -U colophon -d postgres -c 'CREATE DATABASE colophon_r2'
      COLOPHON_TEST_DATABASE=colophon_r2 scripts/migrate.sh
      ```

      The server, the Rust stack tests and the Playwright configs follow the same variable.
- [ ] If you copy a database to try this on, take the copy at 0015 and keep the original.
- [ ] Note the branch and commit you build (`git rev-parse --short HEAD`).

## Are you affected?

```sql
select version, description, success from _sqlx_migrations where version >= 15 order by version;
```

| What you see | What it means |
|---|---|
| only `15 draft titles` | `main`'s schema: nothing from the branches yet. |
| `15 slugs` | Migrated by the first commit of the 3.6 branch (`10f3c65`), when slugs were 0015. Neither `main` nor any branch can migrate it now: drop it and start again. |
| `16 slugs` | The 3.6 branch, or a later one. |
| `17 roles` | R.1 or R.2. |
| `18 custom roles` | R.2. |

## What changes for you

- **Editors:**
  - an article can have a **URL name** (a slug): the part of a web address that names it. It is 1 to 120 lowercase
    letters and digits joined by single hyphens, refused rather than rewritten when it isn't one, and no two articles
    in a collection share one, among drafts or among what is published (3.6, D-054);
  - the Studio's content-type builder checks each change against stored content: removing a field, making one
    required or narrowing a limit is refused, with the first 20 entries in the way, while any draft or published
    version wouldn't fit (3.6, D-055);
  - owners can create roles of their own, choosing what each may do, besides the four built-in ones (R.2). R.1 alone
    changes nothing you can see.
- **Developers:**
  - a `slug` field type; a URL name that is taken answers `409` with `details.reason` `slug_taken` and the entry in
    the way; a content-type change that stored content doesn't fit answers `422` with `needs_migration`;
  - `GET`, `POST`, `PUT` and `DELETE` on `/api/v1/projects/{project}/roles` and `/roles/{role}` (R.2). A role has a
    version: changes and deletes need `If-Match`, a stale one answers `409` `stale_role`, and deleting a role someone
    holds answers `409` `role_in_use`. A permission is an action and a subject, a collection or `*`.
- **Operators:** no new settings. Three migrations, and a separate database while they are unmerged.

## Steps

1. Stop every `colophon-server` process that uses the database.
2. Check out and build the branch:

   ```sh
   git fetch origin
   git switch r/r.2-custom-roles
   cargo build --release --locked -p colophon-server -p colophon-cli
   npm ci && npm run build -w @colophon/studio
   ```

3. Apply the migrations to the separate database, as its owner:
   - **0016 slugs:** a checked `slug` column on revisions, `draft_slug` on entries, `slug` on publications, and two
     partial unique indexes (one among drafts, one among open publications). Every existing slug is empty, so nothing
     is backfilled.
   - **0017 roles:** the `roles` and `role_permissions` tables, the `colophon_seed_roles` function, the four built-in
     roles seeded into every existing project, and memberships and invitations made to reference a role of their
     project. Row-level security on `projects` is lifted for the seeding and forced again in the same transaction.
   - **0018 custom roles:** a `version` and `updated_at` on roles, triggers that refuse any change to the owner role or
     its permissions, invitations no longer tied to a role by foreign key, and the application allowed to change and
     delete roles.

   ```sh
   COLOPHON_TEST_DATABASE=colophon_r2 scripts/migrate.sh
   ```

4. Start the server against the same database (`scripts/local-env.sh` sets both URLs when
   `COLOPHON_TEST_DATABASE` is set), then wait until `colophon doctor` exits 0.

## Check it worked

- `_sqlx_migrations` shows 16 to 18, each `success`.
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
- In the Studio, a project's settings list its roles, each with a page showing what it may do and who holds it, and
  an owner can create and edit roles there (R.2).
- In a content type with a URL-name field, saving a second article with a URL name the first already has is refused,
  naming the first.

## Rolling back

- **`main` refuses the database.** Its `colophon admin migrate` stops with `migration 16 was previously applied but is
  missing in the resolved migrations`, and its server may fail on the changed constraints. There are no down
  migrations: go back to the copy you kept at 0015, or drop the separate database.
- The same holds between branches: an R.2 database can't go back to R.1's or 3.6's binaries.

## Known issues

- **Branch migrations change.** A database migrated by an earlier commit of a branch stops with `migration N was
  previously applied but has been modified`. It has happened twice: slugs were `0015_slugs.sql` in `10f3c65` before
  `main` took 0015 for draft titles, and `0018_custom_roles.sql` changed (its comment) in `5cc99e4` after `ff88120`.
  sqlx checks every byte, comments included. Drop the database and migrate again.
- **Merge order.** R.1 contains 3.6 and R.2 contains R.1, so the numbers hold if they merge in that order. If they
  merge otherwise, they may be renumbered.
- **Field lists and conditions aren't used yet.** `role_permissions` has `fields` and `condition` (`own`) columns, but
  the API refuses both until R.3 enforces them; that step is planned, not built.
- **Invitations and deleted roles.** Since 0018 an invitation's role isn't a foreign key; inviting holds a shared
  advisory lock on the role, and deleting a role takes it exclusively and refuses while a pending invitation offers it.

The migrations, on the R.2 branch:
[0016](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/crates/colophon-db/migrations/0016_slugs.sql),
[0017](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/crates/colophon-db/migrations/0017_roles.sql),
[0018](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/crates/colophon-db/migrations/0018_custom_roles.sql).
Decisions D-054, D-055 and D-058 are in
[that branch's DECISIONS.md](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/DECISIONS.md).
