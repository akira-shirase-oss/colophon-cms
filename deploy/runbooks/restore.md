# Runbook: restore (D-126)

The restore half of the recovery strategy (spec §33, module 18): bring a captured backup back
on **clean, disposable infrastructure** in six ordered stages, so that serving resumes only
after a new recovery epoch, the deletion-ledger replay and operator reconciliation — never
before (invariants O2 and O3). A backup existing is not proof of a working restore; this
runbook is the procedure that turns a verified artifact into a served database, and
`scripts/restore.sh drill` is the dated evidence it works.

## The six stages

```sh
scripts/restore.sh BACKUP_DIR [options]    # run every stage still pending
scripts/restore.sh BACKUP_DIR --step STAGE # run exactly one stage
scripts/restore.sh BACKUP_DIR --from STAGE # run that stage onward
scripts/restore.sh status DIR              # print the stage markers
scripts/restore.sh teardown DIR [--purge]  # drop the scratch containers and volume
```

Each stage writes a marker into the workspace (`restores/<backup-stamp>/` by default,
`--into DIR` to choose): `stages/<name>.done` holds the stage's result (epochs advanced,
jobs held, replay watermarks, the serve address); `stages/<name>.blocked` records a stage
that ran and refused, with the reason. A stage refuses to run while any earlier marker is
missing or blocked, and a done stage is never silently rerun — `--force` is the explicit
rerun. **The markers are the ordering:** a doubled epoch or a re-held outbox is exactly the
silent rerun they exist to prevent.

1. **prepare** — verifies the payload's SHA-256 against `manifest.json`, decrypts
   `base.tar.gz.<enc>` through the key file the manifest references (age or gpg, whichever
   *made* the backup), and unpacks the data directory into a fresh Docker volume. The
   scratch Postgres runs as its own container on its own 127.0.0.1 port — nothing touches
   the live stack's directories, ports or volumes.
2. **verify** — `pg_verifybackup` on the restored bytes **before the cluster first starts**
   (a started cluster rewrites files), then the schema check: the restored database's
   applied migrations against the manifest's list and this binary's embedded set. A backup
   carrying a migration this binary doesn't know is refused — newer data on older code is
   guessing. A schema behind the binary reports `pending` and passes; nothing migrates
   inside verify — `admin migrate` is the deliberate step after restore.
3. **epoch** — `colophon admin restore epoch` advances every environment's epoch exactly
   once. The recovery epoch is what invalidates the old timeline: cache generations,
   delivery scope keys and worker leases from before the incident can never collide with
   what follows (D4).
4. **ledger** — for every `--ledger FILE` export, checks the file's header, then replays the
   interval the restored `deletion_ledger` lacks (anti-joined by row id, not `id > max` —
   ids drawn out of order would hide real rows beneath the watermark). Files are replayed
   in `after` order; a file starting above the coverage reached so far, or a row that
   conflicts on replay, blocks the stage — that is the "can't be reconstructed" case O2
   keeps serving off for. Export the ledger off-host from the backup schedule; a restore
   without one is an immediate `blocked` marker.
5. **reconcile** — `admin restore hold` freezes every unfinished job as `held` (a status no
   claim, retry or expiry sweep touches; leases clear) and `admin restore jobs` prints the
   inventory the operator reconciles: the held outbox with the state each job was taken
   from (`was=running` marks in-flight attempts whose external effect is *unknown* — F13's
   "was it delivered?"), plus due schedules, open asset/bulk/content-migration work.
   **Nothing dispatches until the operator says so:** `--resume-jobs` is the explicit
   enable — `admin restore resume` returns held jobs to `pending` with a fresh lifetime,
   done only after the reconciliation is answered in the affirmative.
6. **serve** — only after all five markers: starts `colophon-server --role api` against the
   scratch database with a scratch Valkey (the live cache's entries belong to the old
   timeline) and waits for `/readyz`. That is the gate that finally serves.

Every `admin restore` subcommand runs on the migrator's connection to the *restored*
database — `COLOPHON_MIGRATION_DATABASE_URL` in the environment, never a credential on the
command line — because these statements name no project context and row-level security does
not force on the owner.

## Reconciliation, concretely

`held` is a real state, not a flag: the claim query never selects it, so no restored worker
can dispatch while it stands, and resume — not expiry, not a retry — is the only way back.
The operator's checklist against `admin restore jobs` output:

- **`was=running` jobs:** their attempt may already have landed. The idempotency key is
  stable (`colophon:<project>:<env>:<generation>:<destination>`), so a resumed resend lands
  once — but confirm the destination (or a replacement) is the intended one before resuming.
- **Due schedules:** a publish scheduled while the system was down fires immediately on
  resume. Check the list is still wanted.
- **Asset/bulk/migration work:** open counts are reported; resume what the operator keeps,
  cancel what the incident made wrong.
- **External effects:** CDN purges and site deployments that fired on the old timeline are
  in the reconciliation notes, not the database — the ledger knows deletions; the operator
  knows the CDN.

## The drill

```sh
scripts/restore.sh drill [--keep]
```

Against the live stack (`COLOPHON_TEST_DATABASE`, default `colophon`): seeds an in-flight
job leased to a worker that won't exist after restore, captures a real backup, deletes a
victim entry through `colophon_delete_entry` and edits a second entry **after** capture,
exports the ledger, then runs all six stages and asserts the restored database shows: the
epoch advanced exactly once, the victim tombstoned by replay, the post-backup edit absent,
the in-flight job `held`, and `/readyz` serving. The dated record lands in
`perf/results/<stamp>-restore-drill-<machine>.json` with the capture-to-verified-serving
wall time — laptop numbers about the reference profile, never a capacity claim. `--keep`
leaves the scratch stack up for inspection; `teardown` removes containers and the volume.

## Failure modes worth knowing

| Marker | Meaning | Response |
|---|---|---|
| `prepare.blocked`: payload SHA-256 mismatch | The stored bytes are not what was captured | Do not restore; investigate storage and re-capture |
| `verify.blocked`: `pg_verifybackup` rejected | The decrypted tree disagrees with PostgreSQL's manifest | The artifact is corrupt; restore from an earlier backup |
| `verify.blocked`: unknown migrations | The backup is newer than this binary | Upgrade the binary first, then restore again |
| `ledger.blocked`: no `--ledger` files / a gap | The interval from the backup's watermark can't be reconstructed | Fetch the missing exports; serving stays off until covered |
| `ledger.blocked`: replay conflicts | A ledger row can't be reapplied to this state | Read `log/ledger.jsonl`; resolve the conflicting object, then `--step ledger` |
| `serve.blocked`: `/readyz` never came up | The scratch server or its dependencies failed | `log/server.log`; fix, then `--step serve` |
