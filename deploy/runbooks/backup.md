# Runbook: backups (D-125)

Encrypted daily base backups plus continuous WAL archiving — the capture half of the
point-in-time-recovery strategy (spec §32, module 18). The targets are a 15-minute
recovery-point objective and a one-hour recovery-time objective **on the reference profile**;
the drills record real wall-clock numbers against the local stack, which are evidence about
that profile, not a promise about other hardware.

**A backup existing is not proof of a working restore.** This runbook covers capture,
archiving and verification. The restore procedure — clean infrastructure, a new recovery
epoch, the deletion-ledger replay, reconciling external effects before dispatch — is a later
P6 slice.

## What runs all the time

Postgres archives every completed WAL segment (`wal_level=replica`, `archive_mode=on`,
`archive_timeout=60s` so an idle database still switches within a minute) through
`deploy/postgres/archive-wal.sh`, the stack's `archive_command`. Locally that lands in the
named Docker volume `postgres-wal-archive`, mounted at `/wal-archive` — the stock
`postgres:18` image carries no S3 tooling (no wal-g, aws, curl or python), so the volume is
the stand-in. **Production points the same `archive_command` contract at object storage**
(wal-g or an S3 client): the command takes `%p`/`%f`, and must exit 0 only once the segment is
durably stored, exactly as the local script does. `deploy/postgres/restore-wal.sh` is the
symmetric `restore_command` a PITR restore will use.

The archiver copies to a temp name and renames, treats an identical re-delivery as success
(Postgres retries after restarts), refuses to overwrite a different file, and fsyncs before
reporting success. If the destination is unreachable it fails and Postgres retries forever —
pg_wal growth is the visible alarm; never make the archiver "succeed" by dropping a segment.

`scripts/stack.sh check` proves a switched segment lands in the archive.

## Taking a base backup

```sh
scripts/backup.sh create [root]    # writes <root>/<UTC-stamp>/{base.tar.gz.gpg,manifest.json}
scripts/backup.sh verify <dir>     # decrypt + integrity-check a captured backup
scripts/backup.sh drill [root]     # create + verify + a dated record in perf/results/
```

`create` runs `pg_basebackup` **inside the postgres container** (the host has no PostgreSQL
client tools) as the `colophon_backup` role — `LOGIN REPLICATION`, no table rights, so the
backup credential is separate from every application credential. `-X stream` folds the WAL a
consistent start needs into the copy, and `pg_verifybackup` checks every file against
PostgreSQL's own `backup_manifest` before the bytes leave the container. The backup is
**cluster-wide** (`scope: "cluster"` in the manifest): pg_basebackup copies the data
directory, which is what WAL replay can restore — `pg_dump` was rejected because a logical
dump is not a PITR base. Which database the manifest reports on is
`COLOPHON_BACKUP_DATABASE`, else `COLOPHON_TEST_DATABASE`, else `colophon`.

The gzipped tar streams straight into the encryptor: plaintext never touches the host disk,
and a failed run leaves a dot-temp file, not something that looks like a backup.

## Encryption and the key

- **Tool:** `age` (with an identity file) if `age` and `age-keygen` are on `PATH`, else
  **`gpg --symmetric --cipher-algo AES256`** — gpg is what the maintainer's machine and the
  postgres image already carry; no new dependency.
- **The secret always comes from a file, never a flag** (nothing sensitive in `ps`):
  `COLOPHON_BACKUP_PASSPHRASE_FILE` (gpg) or `COLOPHON_BACKUP_AGE_KEY_FILE` (age) wins;
  otherwise `deploy/backup-passphrase.local` / `deploy/backup-age-key.local`, created on the
  first `create` with mode 600 and covered by `.gitignore`. Losing the file loses every
  backup's decryptability — in production it lives in the secret store, not the repo.
- `manifest.json` records the key *reference* (which file/env var supplied it), never the
  key.

## The manifest

`manifest.json` records what a restore needs to audit or re-fetch: UTC timestamp, the
checkout's git commit and dirty count, the PostgreSQL and pg_basebackup versions, the
non-system schemas, the applied `_sqlx_migrations` list, the encryption key reference and the
payload's SHA-256, the `deletion_ledger` high-water mark (`null` until that slice lands), and
the WAL archive's location, file count and newest segment. The encrypted payload's SHA-256 in
the manifest is the check `verify` runs first.

## Verify before it counts

`verify` is the rule, not a nicety — a backup nobody checked is a hope. It re-reads the
manifest, confirms the payload's SHA-256, decrypts with the referenced key into a scratch
directory, and checks the result is a data directory (`PG_VERSION`, `global/pg_control`,
`postgresql.conf`, a `backup_manifest` that parses with files and a WAL range, streamed WAL
present). With the stack running it then streams **the same decrypted bytes** back into the
container for `pg_verifybackup`, PostgreSQL's own checksum audit — the artifact, not a copy
of it, is what passes or fails.

`drill` writes `perf/results/<stamp>-backup-drill-<machine>.json` with both wall times
(the RTO input) and the verdict `captured-and-verified`. It does not claim the backup
restores — only the restore drill can say that.

## Retention and O5

**Nothing in this stack deletes archived WAL to free disk space (invariant O5).** There is no
cleanup step anywhere in the pipeline; `scripts/stack.sh reset` removes the archive volume
only as part of throwing the whole stack away, and base-backup directories under `backups/`
are deleted by a person, never by a script. When retention is built it will be a documented
job of its own — e.g. a bucket lifecycle rule in production, plus a listed floor (at least
one verified backup older than the newest archive's base) — not an archiver flag. Disk
pressure is handled by the runbook in module 18: stop unsafe admission, measure, apply
measured retention — never silently.

## Failure modes worth knowing

| Symptom | Meaning | Response |
|---|---|---|
| `pg_wal` grows; archiver logging `archive-wal:` errors | The archive destination is failing; Postgres is holding the segment, not losing it | Restore the destination (mount, credentials, bucket); the backlog archives itself. Never clear `pg_wal` by hand. |
| `create` exits mid-run | The capture failed; a `.tmp` dot-file may remain in the stamp directory | Re-run; the partial file is not a backup and is not counted. |
| `verify` fails the SHA-256 | The stored bytes are not what was captured | Do not trust the directory; re-capture and investigate storage. |
| `pg_verifybackup` fails after decrypt | The decrypted tree disagrees with PostgreSQL's manifest | The artifact is not a backup. Treat as corrupt. |
| `verify` says `skipped (stack not running)` | The deep check didn't run | The cheap checks passed; run again with the stack up for the full audit. |
