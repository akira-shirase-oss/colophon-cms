<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon is pre-release: v0.1 is not out, there are no release binaries, and nothing here is for production. So "operations" today means running Colophon on your own machine or as a test install you can throw away, and reproducing the evidence behind it. Running it for real, with backups, restores and an upgrade runbook, is milestone P6. Every page in this section is read from the code on `main` and the step branches, and says which parts exist.

## What a running Colophon is

One Rust binary, `colophon-server`, serves the management API, public delivery and the Studio, and runs the background worker. PostgreSQL holds everything that matters; Valkey is an optional, disposable cache; people sign in through an OpenID Connect provider. The `colophon` command line applies migrations and makes the first administrator.

## The pages

- [Self-hosting requirements](/operations/requirements/): what an install needs (PostgreSQL 18 and its four roles, an identity provider, optional Valkey, Rust and Node to build), and what isn't ready. <Status done="2.3" />
- [The local stack](/operations/local-stack/): PostgreSQL, Valkey, Garage and Keycloak in Docker, bound to `127.0.0.1`, and the checks that prove each one works. <Status done="0.2" />
- [Configuration](/operations/configuration/): every environment variable the server, the worker, the CLI and the scripts read, with its default and limits. <Status done="3.3" />
- [Health checks](/operations/health-checks/): `/livez`, `/readyz` and `colophon doctor`, and what each state means. <Status done="0.3" />
- [The worker](/operations/worker/): running it, leases, retries, locations, and what happens when it crashes. <Status done="3.3" />
- [Upgrades and migrations](/operations/upgrades/): every database migration, how to apply them, and the upgrade guides. <Status done="3.5" />
- [Backups and restore](/operations/backups/): what P6 plans, and what a plain dump does and doesn't give you meanwhile. <Status planned="P6" />
- [Crash tests](/operations/crash-tests/): kill the server and the worker on purpose, and check nothing was lost or doubled. <Status done="2.4" />

Load results, each with the machine and date it ran on, are in [Performance](/resources/performance/).

## Quick reference

| Task | Command |
|---|---|
| Start the local stack and prove it works | `scripts/stack.sh up` |
| Apply migrations, make `dev-owner` the first administrator | `scripts/migrate.sh` |
| Run everything, with a seeded demo project | `scripts/demo.sh` |
| Apply migrations to any database | `colophon admin migrate` |
| Make the first administrator | `colophon admin bootstrap --subject S` |
| Is the server ready? | `colophon doctor`, or `GET /readyz` |
| Run only the worker | `colophon-server --role worker` |
| Everything CI runs | `scripts/check.sh` |

## The one command CI runs

`scripts/check.sh` runs what CI runs, in the same order: the toolchain check, formatting, Clippy with warnings as errors (with and without the fault points), the tests, `cargo doc` with warnings as errors, the architecture rule, both simulator sweeps, `cargo deny`, ShellCheck and the frontend checks. When the local stack is up it adds the integration half on a fresh database, `colophon_check`: the stack's checks, the migrations, the tests that stop and restart services, the crash tests, the testbed and the Studio in Chromium, and short load runs. `QUICK=1` skips that half. Contributors run it before every push:

```sh runnable
scripts/check.sh
```

It takes a while and needs the whole stack. These docs never ask you to run it to try Colophon; the [quickstart](/get-started/quickstart/) is enough for that.

## Not built yet

Module 18 plans the rest for P6: a production topology (a TLS proxy in front, non-root containers, private networks), backups with WAL archiving and a restore drill, the upgrade runbook, metrics and trace export, budgets and runbooks for PostgreSQL, cache, backlog, credential and disk incidents. <Status planned="P6" />
