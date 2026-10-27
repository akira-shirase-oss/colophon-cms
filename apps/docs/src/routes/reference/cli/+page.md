<script>
	import Status from '#lib/components/Status.svelte';
</script>

`colophon` is the command line for people and scripts. Its normal commands call the same HTTP API as the Studio, with the same permissions, and never need a database password. One separate group, `colophon admin`, is for the operator and talks to PostgreSQL directly. Everything here is read from `crates/colophon-cli/src/` on `main` (the R.2 branch has the same CLI).

There are no release binaries yet. From a clone of the repository, run it through Cargo, or build it once and use `target/release/colophon`:

```sh runnable
cargo run --release -p colophon-cli -- --help
cargo build --release -p colophon-cli
```

`colophon --version` prints `colophon 0.0.0`: every crate is version 0.0.0 until v0.1, so the commit you built from is the only real version.

## Commands

| Command | What it does | Talks to |
|---|---|---|
| `colophon login` | Signs this terminal in, through a code you approve in the Studio | the server |
| `colophon logout` | Revokes the token and forgets it | the server |
| `colophon whoami` | Who you are, on which server, and your projects | the server |
| `colophon content create` | Creates an entry from JSON, safely retried | the server |
| `colophon content list` | Lists a collection's entries, newest first | the server |
| `colophon doctor` | Asks a server whether it is ready, and which dependency is not | the server's `/readyz` |
| `colophon admin migrate` | Applies pending database migrations | PostgreSQL, as the table owner |
| `colophon admin bootstrap` | Makes the installation's first administrator, once | PostgreSQL, as the table owner |

There are no global flags: each command takes its own. `colophon` with no arguments prints its help to stderr and exits 2; `--help` and `--version` exit 0.

## Output and exit codes

- Without `--json`, results are written for people. With `--json`, a successful command prints **exactly one** JSON object on stdout, with `"version": 1` and the `"command"` it answers.
- Errors, progress, prompts and the sign-in link go to stderr, as one `error: …` line for a failure. Only `colophon doctor` also prints a JSON result when it fails; the other commands print nothing on stdout when they fail, so a script reads the exit code and stderr.
- `colophon logout` and `colophon admin migrate` have no `--json` flag.

| Code | Meaning | From the server | On your machine |
|---|---|---|---|
| 0 | Success | `2xx` | |
| 2 | Usage or validation error | `400`, `422`, `428` and every other `4xx` not listed below | a bad or missing flag (clap); content that isn't JSON; an unreadable `--file`; a URL that isn't `http(s)`; a damaged `credentials.json` |
| 3 | Unauthorized or denied | `401`, `403`, `404` | not signed in for that profile; a project you aren't a member of; a sign-in code that expired |
| 4 | Conflict | `409` | `admin bootstrap` for a second person |
| 5 | Transient failure, or unavailable | `429`, `5xx` | no answer, or no answer in time; the database unreachable (`admin`) |
| 6 | Internal failure | anything else, such as a redirect (the CLI follows none) | an incomplete answer from the server; a credentials file it can't write; a migration that failed |

`404` counts as denied because a project you can't see answers exactly like one that doesn't exist. A `409` includes reusing an idempotency key for a different request, and, on the 3.6 branch, a URL name another entry already has. <Status building="3.6" />

The server's error message is printed with its status, and with the JSON pointer of the field at fault when there is one: `error: … (at /title) [HTTP 422]`. Every request gives up after 30 seconds (5 to connect).

## Signing in

### How device sign-in works

The CLI never sees your password. It signs in the way a television does (the OAuth device grant, RFC 8628), approved through your Studio session:

1. `POST /auth/device` with the terminal's name. The server answers with a secret to poll with, a short code (`XXXX-XXXX`, from 28 letters and digits that can't be confused), a link to `/studio/device?code=…` on its public URL, a poll interval (2 seconds) and a lifetime (10 minutes).
2. The CLI prints the link and the code on stderr. You open the link in a browser where you are signed in to the Studio, check that the page shows the same code, and approve.
3. Meanwhile the CLI polls `POST /auth/device/token` at the interval. A poll that gets no answer is sent again. Once you approve, the server hands out a personal access token, exactly once, valid for **30 days**.
4. The CLI stores the token, then reads `/me` to tell you who you are.

Only a browser session can approve, so a token can never mint more tokens. The server stores only hashes of the device secret and the token (migration 0004). The routes are in [Sign-in and sessions](/reference/api/auth/).

### colophon login

```sh runnable
cargo run --release -p colophon-cli -- login --server http://localhost:58000
```

| Flag | Default | Meaning |
|---|---|---|
| `--server URL` | `http://localhost:58000`, or `COLOPHON_SERVER` | The server's base URL. |
| `--profile NAME` | `default` | The name to keep these credentials under. |
| `--name TEXT` | `colophon CLI` | What the approval page calls this terminal. |
| `--json` | | Print one JSON result. |

```json
{
	"version": 1,
	"command": "login",
	"status": "signed_in",
	"server": "http://localhost:58000",
	"profile": "default",
	"person": "Dev Owner",
	"expires_at": "2026-11-26T10:00:00Z"
}
```

`person` is your display name, or your id when the provider gave no name. If the code expires before anyone approves it, the command exits 3: run it again.

### colophon logout

`colophon logout [--profile NAME]` revokes the profile's token on the server (`POST /auth/logout`), then removes it from `credentials.json`. A token the server no longer accepts (expired or already revoked) is removed all the same. It prints `Signed out of SERVER (profile NAME).` and has no `--json`.

### colophon whoami

`colophon whoami [--profile NAME] [--json]` shows your name, the server, whether you are an administrator, your subject at the identity provider and your projects with your role in each. The operator needs that subject for `colophon admin bootstrap`.

```json
{
	"version": 1,
	"command": "whoami",
	"server": "http://localhost:58000",
	"me": {
		"id": "…",
		"issuer": "http://localhost:58080/realms/colophon",
		"subject": "00000000-0000-4000-8000-00000000c001",
		"display_name": "Dev Owner",
		"administrator": true,
		"projects": []
	}
}
```

`me` is exactly the `data` of `GET /me`; [Sign-in and sessions](/reference/api/auth/) describes each project's fields.

### Where credentials are kept

Tokens are kept in `credentials.json`, in the first of these that is set and not empty:

1. `$COLOPHON_CONFIG_DIR`
2. `$XDG_CONFIG_HOME/colophon`
3. `$HOME/.config/colophon`

With none of them set, `login` exits 2. The file is created readable by its owner only (mode `0600` on Unix), and holds one entry per profile:

```json
{
	"profiles": {
		"default": { "server": "http://localhost:58000", "token": "…", "expires_at": "2026-11-26T10:00:00Z" }
	}
}
```

A profile remembers its server, so the content commands and `whoami` take only `--profile`. Tokens never appear in arguments or output. The OS keyring that module 08 describes isn't used yet. <Status planned="not scheduled" />

## Content

Both commands take the collection (the content type's name, for example `articles`) and these flags:

| Flag | Default | Meaning |
|---|---|---|
| `--project ID_OR_SLUG` | required | The project's id or slug. |
| `--env NAME` | `production` | The environment. |
| `--profile NAME` | `default` | Which signed-in profile to use. |

The CLI first lists your projects (`GET /api/v1/projects`) and finds the one whose id or slug matches; if you aren't a member of it, it exits 3 before sending anything else.

### colophon content create

```sh runnable
cargo run --release -p colophon-cli -- content create articles --project my-site --file article.json --json
```

| Flag | Meaning |
|---|---|
| `--data JSON` | The content as JSON, or |
| `--file PATH` | a file holding it. Exactly one of the two. |
| `--idempotency-key KEY` | Reuse the key of an earlier attempt that got no answer: it can't create twice. |
| `--json` | Print one JSON result. |

It sends `POST …/collections/COLLECTION/entries` with an `Idempotency-Key`: yours, or a new `cli-` key. When a request gets no answer, or the server answers `429` or `5xx`, it waits 1, 2, then 4 seconds and sends the same request with the **same key**, four attempts in all. If all four fail it exits 5 and prints the key; running the command again with `--idempotency-key` and that key is equally safe. An answer with `Idempotent-Replayed: true` means an earlier attempt had already created the entry, and the result says `replayed`.

```json
{
	"version": 1,
	"command": "content create",
	"status": "created",
	"idempotency_key": "cli-0199…",
	"entry": { "id": "…", "collection": "articles", "revision": "…", "etag": "\"…\"", "content": { "title": "Hello" } }
}
```

Content that doesn't fit the content type exits 2, with the field's pointer; a role that can't create entries exits 3; a key already used for different content exits 4. The text output is `Created articles ID (TITLE)`, or `Already created: …` for a replay.

### colophon content list

```sh runnable
cargo run --release -p colophon-cli -- content list articles --project my-site --limit 50 --json
```

`--limit N` takes 1 to 100 (default 20). Entries come newest first. The command reads one page: the JSON result carries `next_cursor`, but there is no flag to follow it yet.

```json
{
	"version": 1,
	"command": "content list",
	"collection": "articles",
	"entries": [{ "id": "…", "title": "Hello", "revision": "…", "updated_at": "2026-10-27T09:00:00Z" }],
	"next_cursor": null
}
```

The text output is one line per entry (id, last change, title), or `No entries in COLLECTION.`

## Health

### colophon doctor

```sh runnable
cargo run --release -p colophon-cli -- doctor --server http://localhost:58000 --json
```

It reads the server's `/readyz`, so it reports exactly what a load balancer sees, and shows every check the server reports, including ones added after the CLI was built.

| Flag | Default | Meaning |
|---|---|---|
| `--server URL` | `http://localhost:58000`, or `COLOPHON_SERVER` | The server's base URL. |
| `--timeout-ms N` | 5,000 | Give up after this long: 1 to 120,000. |
| `--json` | | Print one JSON result, on success and on failure. |

```json
{ "version": 1, "command": "doctor", "server": "http://localhost:58000", "status": "ready", "role": "all", "checks": { "cache": "up", "database": "up" } }
```

| Exit | When | `status` in the JSON |
|---|---|---|
| 0 | The server is ready | `ready` |
| 5 | It answered, but isn't ready | `degraded`, `starting`, `not_ready` or `draining` |
| 5 | No answer within the timeout, or no connection | `unreachable` |
| 2 | `--server` isn't an `http://` or `https://` URL | `invalid_server` |
| 6 | It answered something that isn't a readiness report | `unexpected_reply` |

`degraded` exits 5 even though the server still takes traffic: a script asking "is everything up?" should hear no. On a failure the JSON has `error` instead of `role` and `checks`. [Health checks](/operations/health-checks/) explains each state.

## Operator mode

`colophon admin` is the only part of the CLI with direct infrastructure access. It needs the **migrator's** connection URL, the role that owns the tables, never the application's: `--database-url` or `COLOPHON_MIGRATION_DATABASE_URL` (its value is never echoed). It opens one connection, waits at most 10 seconds for it, and allows a statement up to 5 minutes. A URL that doesn't parse exits 2; a database it can't reach exits 5.

### colophon admin migrate

`colophon admin migrate [--database-url URL]` applies every migration the binary knows that the database hasn't had, in order, and prints `migrations up to date (N known)`. The migrations are compiled into the binary, so build it from the commit you are moving to. A failed migration exits 6 with the whole error chain, and the database keeps every migration before it. [Upgrades and migrations](/operations/upgrades/) lists them all.

### colophon admin bootstrap

```sh
colophon admin bootstrap --subject 00000000-0000-4000-8000-00000000c001 --json
```

Makes one person the installation's first administrator, the only kind of person who can create projects. It binds them to an identity provider's issuer and their subject there, never to an email address; they don't have to have signed in yet.

| Flag | Default | Meaning |
|---|---|---|
| `--subject S` | required | The person's subject at the provider (`colophon whoami` shows yours). 1 to 255 characters, no control characters. |
| `--issuer URL` | `COLOPHON_OIDC_ISSUER` | The provider's issuer: `https://`, or `http://` on `localhost` or `127.0.0.1`. Required if that variable isn't set. |
| `--database-url URL` | `COLOPHON_MIGRATION_DATABASE_URL` | The migrator's connection URL. |
| `--json` | | Print one JSON result. |

It runs once per installation. Run again for the same person, it changes nothing and exits 0 with `already_bootstrapped`; for anyone else it is refused and exits 4. The `installation` table's triggers refuse any change afterwards, even from the table owner.

```json
{ "version": 1, "command": "admin bootstrap", "status": "bootstrapped", "identity": "…", "issuer": "http://localhost:58080/realms/colophon", "subject": "00000000-0000-4000-8000-00000000c001" }
```

On the local stack, `scripts/migrate.sh` builds the CLI, runs `admin migrate`, then bootstraps `dev-owner`:

```sh runnable
scripts/migrate.sh
```

## Environment variables

| Variable | Read by | Meaning |
|---|---|---|
| `COLOPHON_SERVER` | `login`, `doctor` | The default `--server`. |
| `COLOPHON_CONFIG_DIR` | `login`, `logout`, `whoami`, `content` | Where `credentials.json` lives; then `XDG_CONFIG_HOME`, then `HOME`. |
| `COLOPHON_MIGRATION_DATABASE_URL` | `admin migrate`, `admin bootstrap` | The migrator's connection URL. |
| `COLOPHON_OIDC_ISSUER` | `admin bootstrap` | The default `--issuer`; the server reads the same variable. |

## Not built yet

The merged command contract ([module 08](/design/modules/08-cli/), D-008) lists more. None of these exist:

| Commands | Status |
|---|---|
| `release validate`, `release publish`, `release status`, `jobs inspect`: listed for P2 in module 08, which finished without them | <Status planned="not scheduled" /> |
| `integration test`, `doctor --env` with profiles that carry an environment: listed for P3 in module 08, but in none of P3's steps | <Status planned="not scheduled" /> |
| `--jsonl` streams, `content import`, `content get --generation`, `release history`, `export`, scoped machine credentials | <Status planned="P5" /> |
| `init`, `migrate plan`, `migrate apply` | <Status planned="P6" /> |
| `schema validate`, `schema diff`, `dev` | <Status planned="not scheduled" /> |
| API tokens for scripts, with the same permissions as a role | <Status planned="R.4" /> |

## Design

[Module 08, the CLI](/design/modules/08-cli/); decisions D-008, D-041 (device sign-in) and D-043 (the first administrator) in the [decisions](/design/decisions/). The exit-code contract is tested in `crates/colophon-cli/tests/exit_codes.rs` and `tests/doctor.rs`, and the whole sign-in and create flow against a real server in `crates/colophon-server/tests/cli.rs`.
