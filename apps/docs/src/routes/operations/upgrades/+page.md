<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon's database schema changes through numbered SQL files in `crates/colophon-db/migrations/`. There are 15 on `main` and three more on branches being built. They are forward-only: there are no down migrations, so the way back from one is a copy of the database taken before it. There are no versioned releases yet, so an upgrade today means moving from one commit to a later one; the [upgrade guides](/operations/upgrades/#upgrade-guides) group the migrations by milestone.

## How migrations are applied

The migration files are compiled into the binaries (`sqlx::migrate!`), so a build knows exactly the migrations of the commit it was built from. `colophon admin migrate` applies the ones the database hasn't had, in order, as the **migrator**: the role that owns the tables, never the server's login.

On the local stack, one script builds the CLI, applies the migrations and makes `dev-owner` the first administrator (a no-op once done):

```sh runnable
scripts/migrate.sh
```

It reads `deploy/local.env`, and with `COLOPHON_TEST_DATABASE` set it migrates that database instead of `colophon` (the database must already exist; `scripts/check.sh` and `scripts/demo.sh` create theirs).

Against any other database, run the CLI with the migrator's URL:

```sh
cargo build --release --locked -p colophon-cli
COLOPHON_MIGRATION_DATABASE_URL=postgres://colophon:PASSWORD@db.internal:5432/colophon \
  target/release/colophon admin migrate
```

It prints `migrations up to date (N known)` and exits 0, or exits 6 with the whole error chain. What the migration library, sqlx 0.9, does underneath:

- it holds a PostgreSQL advisory lock while it works, so two migrators queue rather than interleave;
- each migration runs in one transaction (none of Colophon's opts out), so one that fails or is interrupted leaves nothing behind, and every migration before it stays applied;
- it records each one in the `_sqlx_migrations` table with a checksum of the file, and refuses to go on if an applied migration's file has changed (`migration N was previously applied but has been modified`) or if the database has one the binary doesn't know (`migration N was previously applied but is missing in the resolved migrations`).

The server never migrates on its own, and its readiness check doesn't look at migrations: a new build started against a database that is behind reports `ready` and then fails requests. Always migrate first.

## Where a database stands

```sql
select version, description, installed_on, success
from _sqlx_migrations
order by version;
```

The highest `version` with `success` true is the last migration applied. The tables below say which milestone that is.

## An upgrade, step by step

The migrations aren't written yet for an old and a new server running side by side (module 02's expand, backfill and contract is P6). For example, after migration 0015 an older server can no longer create entries, because `entries.draft_title` is required. So every upgrade has downtime:

1. Stop every `colophon-server` process: the worker first if it runs on its own, then the API.
2. Take a copy of the database you could restore. There is no supported backup yet; [Backups and restore](/operations/backups/) says what a plain `pg_dump` does and doesn't give you.
3. Build the server and the CLI from the commit you are moving to.
4. Apply the migrations with that CLI.
5. Start the API, then the worker.
6. Wait until `colophon doctor` exits 0 (`/readyz` says `ready`), then check what the guide for that range says.

## Every migration

| # | File | Step | Status | What it does |
|---|---|---|---|---|
| 0001 | `tenancy_and_identity` | 1.1 | <Status done="1.1" /> | Identities, sessions and sign-in attempts (tokens stored only as hashes), projects, environments and memberships. Row-level security denies every project row unless the transaction has set whose request it is and for which project; creates the `colophon_api` role if it is missing. |
| 0002 | `entries_and_revisions` | 1.2 | <Status done="1.2" /> | Entries with a draft pointer, insert-only revisions, and idempotency records. A save moves the pointer only from the revision it started from. |
| 0003 | `content_types` | 1.5 | <Status done="1.5" /> | Collections and insert-only content-type versions. Every environment gets the built-in `articles` type, version 1, and every existing revision is backfilled with its collection and schema version. |
| 0004 | `device_sign_in` | 1.6 | <Status done="1.6" /> | Device sign-ins and personal access tokens for the CLI, stored as hashes; tokens are revoked, never deleted. |
| 0005 | `members_and_invitations` | 1.7 | <Status done="1.7" /> | Owners may change and remove members; a trigger checked at commit refuses to leave a project without an owner. Single-use invitation links, stored as hashes. |
| 0006 | `administrators` | 1.8 | <Status done="1.8" /> | The permanent `installation` row (who was bootstrapped, once) and `administrators`. Only administrators may create projects; the application can read both tables and write neither. |
| 0007 | `changesets` | 2.1 | <Status done="2.1" /> | Changesets (publish requests) that pin exact revisions, and insert-only reviews that count only while the changeset keeps the hash they approved. |
| 0008 | `releases` | 2.2 | <Status done="2.2" /> | Releases, one per generation, and `publications`: each entry's publication history as generation intervals that a `btree_gist` exclusion constraint keeps from overlapping. Grants public delivery's reads to `colophon_delivery`. |
| 0009 | `jobs` | 2.3 | <Status done="2.3" /> | Destinations (webhooks) and jobs: the outbox committed with each release, claimed under leases. Grants the worker's access to `colophon_worker`. |
| 0010 | `one_announcement_in_flight` | 2.4 | <Status done="2.4" /> | An index that lets a claim skip any destination that already has a job in flight, so releases reach it in order. |
| 0011 | `delivery_environment` | 3.1 | <Status done="3.1" /> | A function, owned by `colophon_delivery`, that resolves an environment's current generation in one statement before the cache is asked. |
| 0012 | `unpublish` | 3.2 | <Status done="3.2" /> | Unpublish items in changesets, tombstones, revoked publications and each environment's latest unpublish; replaces the 0011 function to return it. |
| 0013 | `purges` | 3.2 | <Status done="3.2" /> | Destinations may be CDNs; the worker may read what releases closed and took down, to build purge tags. |
| 0014 | `receipts` | 3.3 | <Status done="3.3" /> | Destinations may be probes, optionally pinned to a location; `observations` keeps what every probe attempt saw. |
| 0015 | `draft_titles` | fix after 3.5 (`065e614`) | <Status done="3.5" /> | `entries.draft_title`, backfilled from each entry's draft, with a `pg_trgm` index for title search. |
| 0016 | `slugs` | 3.6 | <Status building="3.6" /> | URL names: a slug on revisions, entries' drafts and publications, unique per collection among drafts and among what is live, by two partial unique indexes. Nothing to backfill. |
| 0017 | `roles` | R.1 | <Status building="R.1" /> | `roles` and `role_permissions`. Every project is seeded with the four built-in roles; memberships and invitations now name one of the project's roles. |
| 0018 | `custom_roles` | R.2 | <Status building="R.2" /> | Roles get a version for `If-Match`; triggers keep the owner role fixed; invitations stop pinning a role by foreign key; the application may change and delete roles. |

Every migration on `main` has stayed byte for byte as it was merged (`git log --first-parent main` shows each file once). The three on branches may still change or be renumbered before they merge.

## Upgrade guides

One guide per milestone's range of migrations, each with an "are you affected?" check, the steps and how to see that it worked:

| Guide | Migrations | Status |
|---|---|---|
| [Upgrading to P1](/upgrading/p1/) | 0001 to 0006 | <Status done="1.8" /> |
| [Upgrading to P2](/upgrading/p2/) | 0007 to 0010 | <Status done="2.4" /> |
| [Upgrading to P3](/upgrading/p3/) | 0011 to 0015 | <Status done="3.5" /> |
| [Unreleased](/upgrading/unreleased/) | 0016 to 0018 | <Status building="R.2" /> |

## Rolling back

- **There are no down migrations.** To undo one, restore the copy you took before it; writes made since are lost.
- **An older build against a newer database isn't supported.** Its `admin migrate` refuses the database (a migration it doesn't know), and its server may fail writes the newer schema requires, as in the 0015 example above.
- **Before the migration ran**, going back is only redeploying the older build.

## Branch databases

A database migrated by a branch is ahead of `main`, and `main`'s binaries refuse it. That is why the scripts and tests follow `COLOPHON_TEST_DATABASE`: a step branch runs against a database of its own and never migrates the one other checkouts share. If a branch renumbered or edited a migration after you applied it (the 3.6 branch's slugs were `0015_slugs` before `main` took 0015), `admin migrate` stops with `was previously applied but has been modified`: drop that database and start it again.

## Adding a migration

For contributors: a migration whose tables new `sqlx::query!` calls use must be applied before the queries compile. `AGENTS.md` has the commands (apply it with `sqlx-cli`, then `cargo sqlx prepare --workspace` and commit `.sqlx/`). Each migration is listed for a line-by-line review in its step's build log.

## Not built yet

- The upgrade runbook: backup verified, rehearsal on staging, additive schema, compatible binaries, smoke checks, backfill, removal after a support window (module 18). <Status planned="P6" />
- Migrations that keep the previous version working during a rolling update, with checkpointed backfills that can stop and resume (module 02), and F10: an interrupted migration or a concurrent schema edit. <Status planned="P6" />
- `colophon migrate plan` and `colophon migrate apply`, and a CI job that rehearses the upgrade from the previous release. <Status planned="P6" />
