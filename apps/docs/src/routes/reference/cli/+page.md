<script>
	import Status from '#lib/components/Status.svelte';
</script>

`colophon` is the command line for people, scripts and (later) agents. It calls the same API as the Studio, with the same permissions, and never needs a database password, except in its separate operator mode. From a clone of the repository, run it through Cargo, or build it once and use `target/release/colophon`:

```sh runnable
cargo run --release -p colophon-cli -- --help
cargo build --release -p colophon-cli
```

## Output and exit codes

- Without `--json`, results are written for people. With `--json`, a command prints **exactly one** JSON object on stdout, with `"version": 1` and the `"command"` it answers. Everything else (progress, prompts, errors) goes to stderr.
- Exit codes say what kind of failure happened, so scripts can branch on them:

| Code | Meaning | From the server |
|---|---|---|
| 0 | Success | `2xx` |
| 2 | Usage or validation error | `400`, `422`, `428` and other `4xx`; a bad flag (clap) |
| 3 | Unauthorized or denied | `401`, `403`, `404` |
| 4 | Conflict | `409` |
| 5 | Transient failure, or the server is unavailable or unreachable | `429`, `5xx`, no connection |
| 6 | Internal failure | anything unexpected, a redirect |

`404` counts as denied because a project you can't see answers exactly like one that doesn't exist.

## Signing in

### colophon login

Signs in a terminal: it prints a link to `/studio/device` and a code; you open the link where you are signed in to the Studio, check the code, and approve. The token it receives lasts 30 days and is kept in `credentials.json`, readable by you only, in `$COLOPHON_CONFIG_DIR`, else `$XDG_CONFIG_HOME/colophon`, else `~/.config/colophon`. Tokens never appear in arguments or output.

| Flag | Default | Meaning |
|---|---|---|
| `--server URL` | `http://localhost:58000`, or `COLOPHON_SERVER` | The server. |
| `--profile NAME` | `default` | The name to keep these credentials under. |
| `--name TEXT` | `colophon CLI` | What the approval page calls this terminal. |
| `--json` | | One JSON result: `status`, `server`, `profile`, `person`, `expires_at`. |

### colophon logout and whoami

`colophon logout [--profile NAME]` revokes the profile's token on the server and forgets it. `colophon whoami [--profile NAME] [--json]` shows who you are, on which server, whether you are an administrator, your projects, and your subject at the identity provider (which the operator needs for `admin bootstrap`).

## Content

Both commands take the collection, then `--project` (the project's id or slug), `--env` (default `production`) and `--profile` (default `default`).

### colophon content create

```sh runnable
cargo run --release -p colophon-cli -- content create articles --project my-site --file article.json --json
```

| Flag | Meaning |
|---|---|
| `--data JSON` | The content as JSON, or |
| `--file PATH` | a file holding it. |
| `--idempotency-key KEY` | Reuse the key of an earlier attempt that got no answer: it can't create twice. |
| `--json` | One JSON result: `status` (`created` or `replayed`), `idempotency_key` and the `entry` (id, collection, revision, ETag, content). |

The command sends one idempotency key per create and reuses it when a request gets no answer, retrying after 1, 2 and 4 seconds. If every attempt fails, it prints the key, so running it again with `--idempotency-key` is just as safe. Content that doesn't fit the content type exits 2; a role that can't write exits 3.

### colophon content list

`colophon content list COLLECTION --project … [--limit N] [--json]` lists entries newest first (`--limit` 1 to 100, default 20). The JSON result has the `entries` (id, title, revision, updated_at) and `next_cursor`.

## Health

### colophon doctor

```sh runnable
cargo run --release -p colophon-cli -- doctor --server http://localhost:58000 --json
```

Asks the server's `/readyz` and prints its status, role and each dependency. Exits 0 only when the server is ready; 5 when it is degraded, not ready or unreachable. `--server` (or `COLOPHON_SERVER`), `--timeout-ms` (default 5,000, at most 120,000) and `--json`.

## Operator mode

`colophon admin` is the only part of the CLI that touches the database directly, and it needs the **migrator's** connection URL (the table owner), never the application's: `--database-url` or `COLOPHON_MIGRATION_DATABASE_URL`.

- `colophon admin migrate` applies pending database migrations.
- `colophon admin bootstrap --subject S [--issuer I] [--json]` makes the installation's first administrator, bound to an identity provider's issuer (default `COLOPHON_OIDC_ISSUER`) and the person's subject there, never to an email address. It works once: run again for the same person it changes nothing (exit 0, `already_bootstrapped`); for anyone else it is refused (exit 4).

`scripts/migrate.sh` runs both against the local stack, bootstrapping `dev-owner`.

## Planned commands

The merged command contract (module 08, D-008) lists more. None of these exist yet:

| Command | Status |
|---|---|
| `release validate`, `release publish`, `release status`, `jobs inspect` | listed for P2 in module 08, not built <Status planned="not scheduled" /> |
| `integration test`, and `doctor --env` with profiles | <Status planned="P3" /> |
| `--jsonl` streams, `content import`, `content get --generation`, `release history`, `export`, scoped machine credentials | <Status planned="P5" /> |
| `init`, `migrate plan`, `migrate apply` | <Status planned="P6" /> |
| `schema validate`, `schema diff`, `dev` | in the contract, not given a milestone <Status planned="not scheduled" /> |
