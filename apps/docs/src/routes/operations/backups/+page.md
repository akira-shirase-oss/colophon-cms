None of this exists yet. Colophon is pre-release, there is no production install to back up, and backups, restore and the restore drill are milestone P6. This page says what P6 plans, so nobody mistakes the gap for an oversight, and what a plain PostgreSQL dump does and doesn't give you meanwhile. Until then, treat any install as disposable and keep your own copies of anything you write in it.

## What P6 plans

From [module 18, operations](/design/modules/18-operations/) and the [roadmap](/design/roadmap/):

- **Backups:** encrypted daily base backups with continuous WAL archiving, so a restore can reach any point in time. The targets on the reference profile are a 15-minute recovery point and a one-hour recovery time, and they count only once a drill has shown them. A SQL dump alone is not this strategy.
- **A backup manifest** recording the schema, the migrations, encryption-key references and the deletion ledger, with backup credentials separate from the application's.
- **Versioned copies of uploaded assets**, once there are assets (P4), checked against the production object store: the local Garage lacks bucket versioning.
- **A restore procedure:** restore onto clean infrastructure with outbound integrations off; check the schema, assets and permissions; register a new **recovery epoch** before serving, so releases from before and after the restore can never be confused; replay the off-host **deletion ledger** before any read API or cache is enabled, so a project deleted after the backup doesn't come back (F14); reconcile external effects (CDN, site deployments, webhooks) before the outbox dispatches anything (F13), never resend blindly; then enable current jobs.
- **A restore drill** onto clean infrastructure, written up with its commands and outcomes. It is part of v0.1's definition of done.
- **The cache isn't backed up.** It holds nothing that can't be rebuilt.

The upgrade runbook belongs to the same milestone; [Upgrades and migrations](/operations/upgrades/) covers what exists for upgrades today.

## What exists that it builds on

- PostgreSQL is the only authority. Valkey and CDNs hold nothing that can't be rebuilt, so a backup is the database, plus the asset store once there is one.
- Each environment already has an epoch, part of every public cache key and every webhook payload, ready for a restore to advance. Nothing advances it yet.
- Pending work is rows in PostgreSQL (the outbox), so a backup holds exactly the deliveries that were owed when it was taken.
- `colophon admin migrate` applies migrations as the database owner, separate from the server's login, and records them in `_sqlx_migrations`, so a restored database says which schema it has.

## Meanwhile: a plain dump

If you want a copy of a test install, `pg_dump` makes one. This is **not supported**: the project hasn't tested it, and it lacks everything the restore procedure above exists for. Taken while the server is stopped:

```sh
pg_dump --format=custom --file=colophon.dump "postgres://SUPERUSER:PASSWORD@HOST:5432/colophon"
pg_restore --dbname="postgres://SUPERUSER:PASSWORD@HOST:5432/colophon_restored" colophon.dump
```

What to know before relying on it:

- **Run it as a superuser** (or a role with `BYPASSRLS`). Every project table forces row-level security even on its owner, and `pg_dump` stops rather than dump a table whose rows a policy would hide.
- **Roles aren't in the dump.** `colophon_api`, `colophon_app`, `colophon_delivery` and `colophon_worker` belong to the PostgreSQL cluster, not the database. Create them, and the owner role, on the target first, as `deploy/postgres/roles.sql` does; otherwise `pg_restore` reports an error for every grant and ownership it can't apply, and the server can't work on what it restored.
- **The extensions** `btree_gist` and `pg_trgm` must be installable on the target.
- **Restoring rewinds the outbox.** Deliveries made after the dump are owed again, and go out again with the same idempotency keys; receivers that deduplicate by key drop them, others don't.
- **Restoring can reuse release numbers.** Releases committed after the dump are gone, and the next publish takes the same generation number for different content, in the same epoch. A website, webhook receiver or CDN that saw the lost release can't tell the two apart. That is F13, and why P6 starts a new epoch.
- **Empty the cache afterwards.** Public cache keys name the epoch and the generation, so for the same reason Valkey could answer a reused generation number with the lost release's content. A CDN in front may hold the same.
- **No point-in-time recovery:** you get the moment of the dump, nothing after it.

## Where this is hosted

Where the maintainer's own site will run is still open (O-012). This page will change when that is decided, and again when P6 builds the real thing.
