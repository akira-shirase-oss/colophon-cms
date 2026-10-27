<script>
	import Status from '#lib/components/Status.svelte';
</script>

This runs the whole of Colophon on your own machine: PostgreSQL, the cache, object storage and a sign-in provider in Docker, then the server with the Studio and the background worker. Every command below is one the repository has today; the blocks marked **Runnable** are checked by a test that fails if a command or flag disappears.

It is a development setup. Its passwords are public, every port listens only on `127.0.0.1`, and none of it is a production install: that guide comes with milestone P6. <Status planned="P6" />

## What you need

- [rustup](https://rustup.rs). It installs the pinned toolchain, Rust 1.99.1, from `rust-toolchain.toml` the first time you build.
- Docker with Compose, plus `curl`, `jq` and `openssl` for the stack's checks. The first start downloads about 1 GB of images.
- Node.js: 22.17 or newer for the frontends; the demo's reference site needs Node 24.
- Optional: [k6](https://k6.io) for load tests (`scripts/install-k6.sh` installs the pinned version) and `cargo-deny`.

If another `cargo` comes first on your `PATH` (Homebrew's, for example), it ignores the pin, and the scripts stop with a toolchain mismatch. Put rustup's proxies first:

```sh
export PATH="$(brew --prefix rustup)/bin:$PATH"
```

## 1. Get the code

```sh
git clone https://github.com/akira-shirase-oss/colophon-cms.git
cd colophon-cms
```

Then install the JavaScript workspace (the Studio, the design system and these docs) from the lockfile:

```sh runnable
npm ci
```

## 2. Start the local stack

```sh runnable
scripts/stack.sh up
```

This starts PostgreSQL, Valkey (the cache), Garage (S3-compatible storage) and Keycloak (sign-in), waits until each is healthy, and then checks that each one really works from your machine, including that each refuses what it must refuse: a wrong password, an anonymous read, a sign-in without PKCE. [The local stack](/operations/local-stack/) lists the ports and what the checks prove.

## 3. Run everything with one command

```sh runnable
scripts/demo.sh
```

The demo builds the server, the command line and the Studio, applies the database migrations, and starts the server (with the worker) on `http://localhost:58000` and the reference website on `http://127.0.0.1:58099`. The first time, it seeds a **Demo site** project: five articles, three published as release 1 and two waiting for review, a webhook that rebuilds the website and a probe that checks it. It uses a database of its own, `colophon_demo`, so it never touches the one other checkouts share.

When it is ready it prints the addresses. Open the Studio at `http://localhost:58000/studio/` and sign in as `dev-owner` with the password `colophon-dev`. Ctrl-C stops everything; the demo data stays for next time. To start again from an empty demo database:

```sh runnable
scripts/demo.sh --reset
```

## 3b. Or run the server by hand

The same, one step at a time, against the stack's main database. First apply the migrations, as the database owner; this also makes `dev-owner` the installation's first administrator, the only person who can create projects:

```sh runnable
scripts/migrate.sh
```

Build the Studio, load the stack's settings into your shell, and start the server with the Studio:

```sh runnable
npm run build -w @colophon/studio
set -a; . deploy/local.env; set +a
COLOPHON_STUDIO_DIR="$PWD/apps/studio/build" cargo run --release -p colophon-server
```

Then open `http://localhost:58000/studio/` and sign in as `dev-owner` (password `colophon-dev`). As the administrator you can create a project; `dev-editor` (same password) can join it through an invitation link. [Configuration](/operations/configuration/) lists every setting in `deploy/local.env`.

## 4. Check that it is ready

In another terminal:

```sh runnable
cargo run --release -p colophon-cli -- doctor
```

`doctor` prints the server's role and the state of each dependency, and exits 0 only when the server is ready (5 when it is degraded, not ready or unreachable). The same report is at `http://localhost:58000/readyz`.

## 5. Use the command line

The CLI signs in with a code you approve in the Studio, so it never needs a password or a database login:

```sh runnable
cargo run --release -p colophon-cli -- login --server http://localhost:58000
```

It prints a link to `/studio/device` and a code. Open the link in the browser where you are signed in, check that the code matches, and approve. Then:

```sh runnable
cargo run --release -p colophon-cli -- whoami
cargo run --release -p colophon-cli -- content list articles --project my-site
```

Replace `my-site` with your project's slug or id. Creating an entry takes its content as JSON; an article has a title and a rich-text body:

```sh runnable
cargo run --release -p colophon-cli -- content create articles --project my-site --json --data '{"title":"Hello from the terminal","body":{"schema":"colophon/rich-text","version":1,"doc":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Written with the colophon CLI."}]}]}}}'
```

The entry appears in the Studio at once: the CLI calls the same API with the same checks. [Command line](/reference/cli/) has every command, flag and exit code.

## Where to go next

- [Editor guide](/editors/): writing, review and publishing in the Studio.
- [Concepts](/concepts/): what happens underneath, and how each guarantee is tested.
- [Crash tests](/operations/crash-tests/) and [load testing](/operations/load-testing/): run the evidence yourself.

To stop the stack, run `scripts/stack.sh down` (keeps the data) or `scripts/stack.sh reset` (removes it).
