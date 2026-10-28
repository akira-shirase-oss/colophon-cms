<script>
	import Status from '#lib/components/Status.svelte';
</script>

This runs all of Colophon on your own machine: PostgreSQL, the cache, object storage and a sign-in provider in Docker, then the server with the Studio and the background worker, and the command line. Every command below is one the repository has today. Blocks marked **Runnable** are checked by a test that fails when a script, subcommand, crate or flag they name disappears.

It is a development setup. Its passwords are public, every port listens only on `127.0.0.1`, and none of it is a production install. <Status planned="P6" />

## What you need

- **Docker** with Compose, plus `curl`, `jq` and `openssl`, which the stack's checks use. The first start downloads about 1 GB of images.
- **Rust through [rustup](https://rustup.rs).** The repository pins its toolchain, Rust 1.99.1, in `rust-toolchain.toml`, and rustup installs it the first time you build.
- **Node.js 22.17 or newer** for the Studio and the other frontends (the workspace's `engines` field). The demo's reference website needs **Node 24**.
- Optional: [k6](https://k6.io) for load tests (`scripts/install-k6.sh` installs the pinned version) and `cargo-deny`.

### If another cargo comes first on your PATH

Only rustup's own `cargo` and `rustc` read `rust-toolchain.toml`. Another one earlier on your `PATH`, such as Homebrew's, ignores the pin and builds with whatever version it is. The scripts check for this (`scripts/check-toolchain.sh`) and stop with a message naming both versions. Put rustup's proxies first, in the shell you build from:

```sh
# rustup installed with Homebrew
export PATH="$(brew --prefix rustup)/bin:$PATH"
# rustup installed from rustup.rs
export PATH="$HOME/.cargo/bin:$PATH"
```

## 1. Get the code

```sh
git clone https://github.com/akira-shirase-oss/colophon-cms.git
cd colophon-cms
```

Install the JavaScript workspace (the Studio, the design system, the reference website and these docs) from the lockfile:

```sh runnable
npm ci
```

## 2. Start the local stack

```sh runnable
scripts/stack.sh up
```

This starts PostgreSQL, Valkey (the cache), Garage (S3-compatible storage) and Keycloak (sign-in), waits until each is healthy, creates the database roles the server and the worker use, and then checks that each service really works from your machine, including that it refuses what it must refuse: a wrong password, an anonymous read, a sign-in without PKCE. [The local stack](/operations/local-stack/) lists the ports and what every check proves.

| Service | Address on your machine |
|---|---|
| PostgreSQL | `localhost:55432`, database `colophon` |
| Valkey | `localhost:56379` |
| Garage (S3) | `http://localhost:53900` |
| Keycloak | `http://localhost:58080`, realm `colophon`; admin console at `/admin` (`admin` / `colophon-dev`) |

## 3. The quick way: one command

```sh runnable
scripts/demo.sh
```

The demo does everything in step 4 for you, against a database of its own (`colophon_demo`), so it never touches the database other checkouts share. It applies the migrations (which makes `dev-owner` the administrator), builds the server, the command line and the Studio, and starts the server with the worker on `http://localhost:58000` and the reference website on `http://127.0.0.1:58099`.

The first time, it signs in as `dev-owner` and seeds a **Demo site** project: five articles, three published as release 1 and two in a publish request waiting for review, a webhook that rebuilds the website on every release, and a probe that checks it. When it is ready it prints the Studio's address, the demo project, the live site and the public API. Ctrl-C stops everything; the demo data stays for next time. To start again from an empty demo database:

```sh runnable
scripts/demo.sh --reset
```

Logs go to `target/demo/server.log` and `target/demo/site.log`.

## 4. Or step by step

The same, one step at a time, against the stack's main database.

### Apply the migrations and make the first administrator

```sh runnable
scripts/migrate.sh
```

The script builds the command line, then runs two operator commands with the database owner's login from `deploy/local.env`:

1. `colophon admin migrate` applies every pending database migration as the migrator role, the role that owns the tables. The server itself never uses that login: it signs in as `colophon_app`, which PostgreSQL's row-level security applies to.
2. `colophon admin bootstrap --subject 00000000-0000-4000-8000-00000000c001` makes `dev-owner` this installation's **first administrator**, the only person who can create projects.

Signing in by itself grants nothing. Bootstrap runs once per installation, bound to an identity provider's issuer and the person's subject (their fixed id at that provider), never to an email address:

| You run it | It says | Exit code |
|---|---|---|
| The first time | `… is now this installation's first administrator; bootstrap is now disabled` | 0 |
| Again, for the same person | `already bootstrapped for …; nothing changed` | 0 |
| For anyone else | `this installation was already bootstrapped for someone else; bootstrap runs once` | 4 |

To run it yourself, load the stack's settings and name the person's subject. The issuer defaults to `COLOPHON_OIDC_ISSUER`, which must be `https`, or `http` on `localhost`; `--issuer` overrides it. On the local stack `migrate.sh` has already bootstrapped `dev-owner`, so this changes nothing and says so:

```sh runnable
set -a; . deploy/local.env; set +a
cargo run --release -p colophon-cli -- admin migrate
cargo run --release -p colophon-cli -- admin bootstrap --subject 00000000-0000-4000-8000-00000000c001
```

On an installation of your own, the first administrator signs in once, runs `colophon login` and then `colophon whoami`, which prints their subject and issuer; the operator bootstraps that subject.

There is a second way, from the browser (step 3.7, D-061). While nobody is an administrator, the server prints a one-time **setup code** to its terminal when it starts, valid for an hour; the first person to sign in to the Studio sees *Finish setting up Colophon*, enters the code, and becomes the administrator. Once anyone is an administrator, by either way, no code is issued and none works. The local stack never shows it, because `migrate.sh` has already bootstrapped `dev-owner`; `scripts/first-run.sh` starts a brand-new installation on port 58400, with a database of its own, to try it. [Onboarding and plain words](/concepts/onboarding/) has the details. <Status done="3.7" />

### Build the Studio and start the server

```sh runnable
npm run build -w @colophon/studio
set -a; . deploy/local.env; set +a
COLOPHON_STUDIO_DIR="$PWD/apps/studio/build" cargo run --release -p colophon-server
```

The server serves the Studio at `/studio/` when `COLOPHON_STUDIO_DIR` names a build, and refuses to start if the folder isn't one. By default it runs both of its roles in one process, `--role all`: the API, and the **worker** that delivers releases to website connections. Running the worker as its own process is covered in [The worker](/operations/worker/). Every setting in `deploy/local.env` is listed in [Configuration](/operations/configuration/).

`scripts/studio.sh` is not a way to start the Studio: it builds the server and the Studio, runs the Studio's browser tests against them on port 58093, and stops.

### If Keycloak predates step 1.8

The development users have fixed ids since step 1.8. A Keycloak container started before that has random ones, and bootstrap then names nobody you can sign in as. One started before step 3.7 has no **Create an account** and doesn't accept the callback on port 58400. Recreate it from the realm file; Keycloak keeps no data of its own:

```sh runnable
docker compose -f deploy/compose.yaml up -d --force-recreate keycloak
```

## 5. Sign in to the Studio

Open `http://localhost:58000/studio/` and press **Sign in**. Keycloak's sign-in page opens, in Colophon's look; afterwards you are back in the Studio. **Create an account** opens Keycloak's form for making one: the development realm lets anyone register, and a new account grants nothing until someone invites it. The development realm has two people already, both with the password `colophon-dev`:

| User | Email | Subject | What they are for |
|---|---|---|---|
| `dev-owner` | `owner@example.test` | `00000000-0000-4000-8000-00000000c001` | The administrator, made by `migrate.sh`. Creates projects and is the owner of each one they create. |
| `dev-editor` | `editor@example.test` | `00000000-0000-4000-8000-00000000c002` | Nobody special until invited. Signed in, they see *You're not in a project yet* and how to ask for an invitation. The tests invite them into a project. |

To be two people at once, use a second browser or a private window for `dev-editor`. Sessions end 12 hours after signing in, or after 2 hours unused; the Studio has no sign-out button yet.

[Your first project](/get-started/first-project/) continues from here.

## 6. Check that it is ready

In another terminal:

```sh runnable
cargo run --release -p colophon-cli -- doctor
```

`doctor` prints the server's role and the state of each dependency. It exits 0 only when the server is ready, and 5 when it is degraded, not ready or unreachable. The same report is at `http://localhost:58000/readyz`; [Health checks](/operations/health-checks/) explains each answer.

## 7. Use the command line

The CLI signs in with a code you approve in the Studio, so it never sees a password or a database login:

```sh runnable
cargo run --release -p colophon-cli -- login --server http://localhost:58000
```

It prints a link to `/studio/device` and a code. Open the link in the browser where you are signed in, check that the page shows the same code, and press **Allow**. The code works for 10 minutes; the CLI keeps the resulting token for 30 days in `credentials.json` (by default in `~/.config/colophon/`), readable only by you, until `colophon logout` revokes it. Then:

```sh runnable
cargo run --release -p colophon-cli -- whoami
cargo run --release -p colophon-cli -- content list articles --project my-site
```

`whoami` prints your name, whether you are the administrator, your subject and issuer, and each project with your role in it. Replace `my-site` with your project's slug or id. [Your first article](/get-started/first-article/) creates an entry from the terminal, and [Command line](/reference/cli/) has every command, flag and exit code.

## Stopping

```sh runnable
# remove the containers, keep the PostgreSQL and Garage data
scripts/stack.sh down
# remove the containers and the data
scripts/stack.sh reset
```

## Where to go next

- [Your first project](/get-started/first-project/), [your first article](/get-started/first-article/) and [your first publish and receipt](/get-started/first-publish/).
- [Crash tests](/operations/crash-tests/) and [performance](/resources/performance/): run the evidence yourself.
