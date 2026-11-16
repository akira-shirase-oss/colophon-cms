# Runbook: upgrades (D-130)

How a Colophon installation moves from one release to the next (spec §32, module 18). The
spec's ordering is the contract: *release notes → verified backup → rehearsal → additive
schema → compatible binaries → smoke checks → backfill → remove old fields only after the
support window*. This runbook is written for someone who did not write the code; every gate
is a command that fails loudly rather than a judgement call.

**What an upgrade is:** a new `colophon-server`/`colophon` pair plus whatever schema
migrations it carries, applied deliberately by `colophon admin migrate` — never at startup,
never by the server itself. The database is the only authority; a binary is always the part
you can swap back, so long as the schema still fits it.

## 0. Read the release notes and the compatibility rules

- Read `CHANGELOG.md` for the target version: breaking changes, required settings, removed
  fields and migration notes are written there, not guessed.
- The CLI refuses a server it doesn't fit (`colophon --version` vs `GET /version`; module
  08): before 1.0, server and CLI share a minor line. Keep them from the same release.
- Migrations are expand-style by discipline (D-130's context: additive steps, backfills
  outside the serving transaction). **Rollback is a binary swap only while the schema still
  fits the old binary** — see the rollback section before assuming it applies.

## 1. Pre-flight checks

```sh
colophon doctor --json                    # every dependency ready/degraded, exit 5 otherwise
colophon admin migrate-status             # baseline: current, or lists what is pending
```

Record the `applied` list — it is the "before" the upgrade is judged against. Check disk
headroom on the backup volume and the Postgres volume before doing anything else (module
18's storage budget: warn at 70%, and a failed upgrade is the worst moment to run out).

## 2. A verified backup, not a hoped-for one

The gate is `verify` passing on a fresh backup, taken against the running stack:

```sh
scripts/backup.sh create [root]            # or let the schedule's `daily` have run
scripts/backup.sh verify backups/<stamp>
```

`verify` re-checks the manifest, the payload's SHA-256, decrypts and re-runs
`pg_verifybackup` while the stack is up. If it fails, stop here — an upgrade is exactly the
change a working restore exists for. The backup's `manifest.json` also records the applied
migrations at capture time: that list is the schema the restore path will compare against.

## 3. List what the new binary will apply

```sh
new-bin/colophon admin migrate-status --json
```

Run the **new** binary's `migrate-status` against the migrator URL
(`COLOPHON_MIGRATION_DATABASE_URL`, never the application's). Three answers:

- `current` — no schema work; the upgrade is a binary swap only.
- `behind` with a `pending` list — those migrations are what step 5 applies. Read them in
  the release notes' terms before continuing.
- `unknown` (exit 4) — **stop.** The database already carries migrations this binary does
  not know: it is older than the schema, not newer. Do not deploy it; do not let it near
  `admin migrate`.

## 4. Rehearse what is rehearsable

`scripts/upgrade-rehearsal.sh` proves the local halves against the real stack and writes a
dated record to `perf/results/`: live `migrate-status`, a scratch database migrated to the
same applied set, the `unknown` refusal at exit 4, and the serving smoke checks. Run it
before a real upgrade; on a staging copy of production it is the rehearsal the spec's
ordering asks for.

What it cannot rehearse is documented, not hidden: the binary swap on a live system
(step 5), a staged roll across several servers, and the support-window cleanup after
backfill. Those are procedure, exercised for real on the day.

## 5. The swap

```sh
# Stop the serving binary (the supervisor's stop, or SIGTERM — the drain gate answers
# draining while in-flight work finishes; COLOPHON_DRAIN_DELAY_MS covers the window).
colophon-server ... &              # the OLD binary, stopped
install -m 0755 new-bin/colophon-server /path/to/colophon-server
install -m 0755 new-bin/colophon        /path/to/colophon
```

Apply the schema **as the migrator**, before serving on the new binary:

```sh
colophon admin migrate             # COLOPHON_MIGRATION_DATABASE_URL; exits non-zero on failure
colophon admin migrate-status      # expect: current
```

Then start the new `colophon-server` under the supervisor. The order is deliberate —
migrate, then serve — because the compatible-binaries rule runs both ways: the old binary
may serve a schema at or behind its knowledge, never ahead of it, and the new binary owns
the same constraint.

## 6. Smoke checks

```sh
curl -fsS http://localhost:PORT/livez          # 200: the process answers
curl -fsS http://localhost:PORT/readyz         # 200: this role may serve
curl -fsS http://localhost:PORT/version        # the expected version/commit
colophon doctor --json                         # dependency view as the CLI sees it
colophon whoami                                # a real authenticated round-trip
```

`/readyz` answering `degraded` is a cache-side note, not a stop — writes stay honest and
the status is published (module 18 "Health"). `not_ready` means the upgrade did not take;
go to the rollback section. Watch the first request log lines: they now carry
`version` and `commit` fields on the request span — check the deployment identity is the
one you meant to deploy (module 18 "Observability").

## 7. Rollback

Decide by the schema, not by the binary.

- **The schema did not move** (`migrate` applied nothing, or you have not run it): swap the
  old binaries back and start them. This is the only cheap rollback, and it is why
  `migrate` is a separate deliberate step.
- **The schema moved forward:** the old binary's `admin migrate-status` now exits 4
  (`unknown`) — proven in the rehearsal. Do not start it against this database. The choices
  are the spec's own:
  - **forward-fix** — keep the new binary, fix what the smoke checks found, redeploy; the
    honest path for anything short of corruption, and it loses no data; or
  - **restore** — `scripts/restore.sh` on the verified backup from step 2, replaying the
    deletion-ledger exports the schedule kept (`deploy/runbooks/restore.md`). Write down
    the data-loss window: everything between the backup's capture and now, then reconcile
    external effects per the restore procedure's reconcile stage.

Removing old fields or contracting a widened schema is a *separate later release*, after
the support window — never part of the same deploy, so the binary-swap rollback above stays
possible for as long as you might need it.

## Failure modes worth knowing

| Symptom | Meaning | Response |
|---|---|---|
| `migrate-status` exits 4 (`unknown`) | The database is newer than this binary | Stop: do not deploy, do not migrate. The rollback gate working as designed. |
| `admin migrate` fails part-way | A migration stopped | It is a transaction per migration — re-run after fixing the cause; `migrate-status` shows where it stands. |
| `/readyz` stays `not_ready` after the swap | A dependency the new binary needs is absent or misconfigured | Read the JSON body for which probe fails; fix and re-check before traffic returns. |
| Smoke checks pass but content looks wrong | A data problem, not a serve problem | Forward-fix first; the restore path is for loss or corruption, and its data-loss window applies. |
| `verify` fails before the upgrade | The backup is not trustworthy | Do not upgrade on a bad backup: re-capture and investigate storage first. |
