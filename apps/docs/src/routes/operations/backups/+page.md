None of this exists yet. Colophon is pre-release, there is no production install to back up, and this page says what milestone P6 plans so nobody mistakes the gap for an oversight. Until then, treat the local stack as disposable and keep your own copies of anything you write in it.

## What P6 plans

From [module 18, operations](/design/modules/18-operations/):

- **Backups**: encrypted PostgreSQL base backups with continuous WAL archiving, and versioned copies of uploaded assets, verified against the production object store (the local Garage lacks bucket versioning).
- **A restore procedure**: restoring starts a new **recovery epoch**, so releases from before and after the restore can never be confused; the deletion ledger is replayed before any read API or cache is enabled, so a project deleted after the backup doesn't come back (F14); and external effects are reconciled before the outbox dispatches anything (F13), never resent blindly.
- **A restore drill** onto clean infrastructure, written up with its evidence. It is part of v0.1's definition of done.
- **An upgrade runbook**, with database migrations that expand, backfill and then contract, so a running version keeps working during an upgrade.
- **Budgets and runbooks** for the outbox, disk and connection pools, with metrics and trace export.

## What exists that it builds on

- PostgreSQL is the only authority. The cache and the CDN hold nothing that can't be rebuilt, so a backup is the database plus the asset store.
- Each environment already has an epoch, in cache keys and webhook payloads, ready for restores to advance it.
- `colophon admin migrate` applies migrations as the database owner, separate from the server's own login.

## Where this is hosted

Where the maintainer's site and these docs will be hosted is still open (O-012 and the docs site's own open decision). This page will change when those are decided, and when the first production deployment of the reference site happens.
