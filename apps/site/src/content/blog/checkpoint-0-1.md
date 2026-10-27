+++
title = "Checkpoint 0.1: the compiler that ignored my instructions"
description = "Pinning a Rust version takes one small file. On my laptop, that file was being silently ignored. So Colophon's first check makes sure its checks are real."
date = 2026-10-25T18:02:19Z

[extra]
# experiment, failure or release
kind = "experiment"
milestone = "P0"
step = "0.1"
short_title = "The compiler that ignored my instructions"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 6

[extra.summary]
goal = "An empty workspace on a pinned compiler, where everything CI checks can also be checked on my laptop."
built = "Five skeleton crates, a pinned Rust 1.99.1, three check scripts, supply-chain rules and CI."
proof = ["6 tests pass", "the toolchain check fails on the wrong compiler", "the architecture check fails when it should", "CI green on the first run"]
code = "akira-shirase-oss/colophon-cms · main @ 68f471b"
+++

You can tell a Rust project which compiler version to use with one tiny file.

I did. My laptop ignored it, and didn't say a word.

## What I built

No CMS features yet. This step is the ground everything else stands on:

- **A workspace with five crates** (Rust's word for packages): the rules, the database layer, the API, the `colophon` command-line tool and `colophon-server`. They're empty for now, apart from `--version` and `--help`.
- **A pinned compiler:** Rust 1.99.1, written down in `rust-toolchain.toml`.
- **Strict lint rules:** no `unsafe` code at all, every Clippy warning is an error, and any warning I silence has to say why.
- **Supply-chain rules** with cargo-deny: only licences I've approved, no OpenSSL, no second async runtime, packages only from crates.io, and a daily check for new security advisories.
- **CI on GitHub,** with every outside action pinned to an exact commit, so nobody can swap one out from under me.
- **One script, `scripts/check.sh`,** that runs exactly what CI runs. If it passes on my laptop, it should pass there.

## How it works (the short version)

The pin file is read by rustup, Rust's version manager. But on my laptop, the `cargo` that runs first comes from Homebrew, and Homebrew's `cargo` has never heard of the pin. It just builds with whatever it has: 1.96.0 instead of 1.99.1. No warning, no error.

So the first script compares the compiler that's *actually* running with the pinned one, and stops if they differ. The error message includes the fix.

The second script guards the most important line in the whole plan. The crate that holds the rules, `colophon-domain`, must never touch the network, the disk or a database. That's what will let the simulator run it with a fake clock and a fake, unreliable network. The script reads the full dependency tree, including dependencies of dependencies, and fails if a banned package shows up anywhere.

## Does it actually work?

- **With my normal setup:** `error: rust-toolchain.toml pins 1.99.1, but this shell runs rustc 1.96.0 and cargo 1.96.0.` Exit code 1. Exactly the point.
- **With rustup's tools first:** `toolchain ok: rustc 1.99.1, cargo 1.99.1`.
- **The architecture check:** a green result proves nothing on its own, because a check that checks nothing is also green. So I pointed it at the command-line crate and put `clap` (which that crate uses) on the banned list. It failed: `error: colophon-cli must not depend on: clap`. Now its green means something.
- **6 tests,** all about promises other programs will rely on. Mistype a flag and `colophon` exits with code 2 and prints nothing to stdout, so a script can tell "bad command" apart from real output. `colophon-server` accepts exactly the three documented roles (`api`, `worker`, `all`) and rejects anything else with code 2.
- **Everything together:** format, Clippy, tests, docs, architecture and cargo-deny all pass locally, with zero warnings.
- **And on GitHub:** the first CI run passed on its first try, both jobs green
  ([run 40219191638](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40219191638)). The laptop and CI
  ran the same script and got the same answer, which was the whole point of this step.

## Where I messed up

- The first full run failed on formatting: one test line was too long for rustfmt's taste.
- Then Clippy failed my test helpers for using `expect()`. I'd told Clippy that tests are allowed to, but it turns out that permission only covers code inside `#[test]` functions, not helper functions next to them. The fix is a labelled exception, `#[expect(clippy::expect_used, reason = "...")]`, which complains if it's ever no longer needed.
- And my first `git push` was refused: the password manager that holds my SSH key wanted a click, and nobody was there
  to give it. One unlock later, it went through.

## What it doesn't do yet

Anything a CMS does. `colophon --version` works, and that's honestly it.

## Up next

**0.2, the local stack:** PostgreSQL, a cache, file storage and sign-in, all started with one command, with a check that each one is really up.
