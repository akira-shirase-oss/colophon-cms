+++
title = "Checkpoint 1.6: the command line saves the same draft"
description = "Colophon gets a command line. You sign in by approving a short code in the Studio, and then the terminal creates and lists exactly the same records the Studio edits, safely retried, with exit codes scripts can rely on. With this, the spec's first slice works end to end."
date = 2026-10-26T01:55:29Z

[extra]
kind = "release"
milestone = "P1"
step = "1.6"
short_title = "The command line"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 166

[extra.summary]
goal = "Do from a terminal what the Studio does, through the same rules, and sign in without pasting passwords anywhere."
built = "colophon login (a code approved in the Studio), whoami, logout, content create and content list, and bearer tokens on every route."
proof = ["the CLI's entry is the Studio's entry", "the first slice works end to end", "1,000,009 token-signed requests, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 6a7157b"
+++

The spec's very first task for Colophon reads: an editor saves a draft through the server, the command line creates the
same record, a conflict comes back safely, and another project is refused. The Studio could do its part since 1.3. This
step builds the command line, and the interesting part is signing in.

## What I built

- **`colophon login`.** It prints a link and a short code like `BCDF-GHJK`. You open the link in a browser where you're
  already signed in, check that the code matches, and click Allow. The terminal is signed in. No password ever touches
  the terminal.
- **`colophon content create` and `content list`.** They use the exact same API the Studio does, so an article created
  from the terminal is the same record you then edit in the Studio, checked against the same content type.
- **`colophon whoami` and `colophon logout`,** which revokes the token on the server, not just on your machine.
- **Exit codes scripts can rely on:** 0 for success, 2 for bad input, 3 for "not allowed", 4 for a conflict, 5 for
  "try again", 6 for a bug. Results go to standard output, everything else to standard error.

## How it works (the short version)

A terminal can't hold a browser's sign-in, so it borrows one. The terminal asks the server for a code; you approve that
code in the Studio, where you're already signed in; the terminal, which has been asking "approved yet?" every couple of
seconds, then receives its own access token, once. It's the same pattern TVs and game consoles use for signing in.

The token is stored hashed on the server, lasts 30 days, and lives in a file only you can read. Only a browser session
can approve a code, so a stolen token can't be used to mint more tokens.

Creating content is retry-safe. Every create carries a one-time key, and if the network drops, the CLI retries with the
same key. If it gives up, it prints the key, so running the command again with that key still can't create a second
entry.

## Does it actually work?

- **The real binaries, end to end:** a test runs `colophon login`, approves the code through a browser session, checks
  that the credentials file is readable only by its owner, creates an article from the terminal and reads the very same
  record through the API, sends it again with the same key and gets the same entry back (one entry, not two), and lists
  it.
- **Every exit code:** bad content exits 2 and names the field, someone else's project exits 3, a key reused for
  different content exits 4. Nothing but results ever reaches standard output.
- **Logging out works on the server:** afterwards the old token gets 401.
- **In a real browser:** the Studio's approval page shows the terminal's name and code, allowing it produces a token
  that works, and the same link afterwards has nothing left to approve.
- **A million requests signed with a token:** 1,000,009 at 1,979 a second, the token checked on every one, and none
  failed (p99 59 ms).
- **166 Rust tests,** 11 Studio tests against the server, 14 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40256165534)).

## Where I messed up

- **A gap the browser tests couldn't see.** The approval page used a value that can be missing on one path. The
  browser tests passed, because they never take that path; the type checker caught it, but it runs as a separate step
  from the Studio's browser tests, so I only saw it afterwards.
- **Fresh code, stale binary.** My first run of the new Studio tests used a server binary built before the content types
  work, so every page waited for an API that didn't exist yet. The project's test script rebuilds first; I had bypassed
  it.

- **I asked the laptop for more than it can do.** The steady load test ran at 2,000 requests a second, but this mix of
  requests tops out around 1,979. It fell behind, failed, and I kept the failed result; it now runs at 1,000.
- **A test failed once and I couldn't tell why.** In the full check, one sign-in came back "sign-in failed". Two reruns
  passed. The server logs the reason, but the test runner was hiding the server's output, so the reason was lost. It
  isn't hidden any more; the next time, it will say.

## Where P1 stands

The first slice works end to end, and the crash tests, the two-editor conflicts and the simulator all pass. Two things
are left before I call the milestone done: entry slugs (they come with publishing, along with the gate's "colliding
slugs" check), and roles, invitations and the very first owner, which is next.

## What it doesn't do yet

- No `content get` or editing from the terminal yet; create and list only.
- Tokens act with your full rights in every project you belong to. Narrow, scoped tokens for scripts and agents are a
  later milestone.
- Starting a sign-in isn't rate-limited yet; old codes are cleared out as new ones arrive.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**1.7, inviting people:** roles, invitations, and a safe way to create the very first owner.
