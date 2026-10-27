+++
title = "Checkpoint 1.8: the first owner"
description = "Signing in to Colophon no longer lets you create anything. The person who runs the server makes the first administrator once, from a terminal, and nobody can do it a second time; only administrators create projects, and everyone else arrives by invitation. With this, the first milestone's proof gate is met."
date = 2026-10-26T03:33:25Z

[extra]
kind = "release"
milestone = "P1"
step = "1.8"
short_title = "The first owner"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 180

[extra.summary]
goal = "Decide who is in charge of a fresh install, safely, once, and make signing in grant nothing by itself."
built = "colophon admin bootstrap, an installation record that can't be undone, administrators who alone create projects, and a Studio form for them."
proof = ["bootstrap works exactly once", "a refused project leaves nothing behind", "1,000,013 requests at the gate, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 2652f43"
+++

Until now, anyone who could sign in could create projects. That's fine on my laptop, but a real install might sit
behind a sign-in provider that lets in far more people than it should, and the first stranger through the door could
start filling it with projects. This step decides who is in charge, and it can only be decided once.

## What I built

- **`colophon admin bootstrap`.** The person who runs the server types one command in a terminal. It names the first
  administrator by their account at the sign-in provider, never by an email address, since an email address doesn't
  prove who someone is. Then bootstrapping is off, for good.
- **Administrators create projects; nobody else does.** Everyone else signs in and sees nothing until a project's
  owner invites them.
- **A "New project" form in the Studio,** for administrators. Everyone else sees how to join a project instead.
- **`colophon whoami` shows your account id,** so an operator can sign in once, copy it, and bootstrap themselves.

## How it works (the short version)

Bootstrapping writes one row to the database: who was made administrator, and when. The database refuses to change
or delete that row, even for the account that owns the tables, so "bootstrap runs once" isn't a promise the code makes;
it's something the database enforces. Running the command again for the same person does nothing; for anyone else,
it refuses.

The web server can read the list of administrators but has no permission to write it. No request, however it signs
in, can make someone an administrator; only the operator's terminal can, and only that once. And the rule that only
administrators create projects is checked twice: by the server, which explains the refusal, and by the database
underneath, in case the server ever forgets.

## Does it actually work?

- **Once, and only once:** a bootstrap for someone who has never signed in creates their account, and their first
  sign-in finds it. Again for the same person changes nothing; for anyone else it's refused. Two bootstraps started at
  the same instant make exactly one administrator.
- **Permanent:** deleting, changing or emptying the bootstrap record fails, even as the database's owner.
- **Against the real server:** the administrator creates a project; someone else, signed in the same way, gets a clear
  403; the bootstrap command exits with the right codes, from the real binary.
- **The database holds the line on its own:** asked directly to create a project for someone who isn't an
  administrator, it refuses. The web server's database account can't add an administrator at all.
- **In real browsers:** the administrator creates a project from the Studio's home page and lands in it as its owner;
  someone else sees how to join instead. No accessibility problems.
- **A million requests at the gate:** 1,000,013 refused creations and "who am I" reads at 3,069 a second, every refusal a 403 and nothing created, none failed (p99 31 ms). The extra lookup on every "who am I" cost little: the signed-in scenario ran at 2,550 a second against 2,671 before, inside its limits.
- **180 Rust tests,** 13 Studio tests against the server, 15 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40322436000)).

## Where I messed up

- **Every test was signing in as the wrong person.** The local sign-in setup has two people, "dev-owner" and
  "dev-editor", and since day one every test created its projects as dev-editor. Once only an administrator could
  create projects, I made dev-owner the administrator and swapped the two across 18 test and load-test files. Two variables called
  `owner` then held the person who wasn't one; I renamed them.
- **The test accounts' ids changed whenever the sign-in server restarted.** It keeps no data between restarts, so an
  administrator bound to an account id would have been a stranger after the next restart. The two test accounts now
  have fixed ids.
- **My shell didn't split words the way I assumed.** Twice I looped over a list in zsh as if it were bash; the loop got
  one long string, and nothing ran. The first time the files simply weren't edited; the second time four load runs
  exited straight away with a usage error. Both showed up immediately, but it's the same mistake twice.

## Where P1 stands

The proof gate is met. Crashes before and after a save neither lose nor duplicate a draft, two editors can't overwrite
each other, another project is invisible, the Studio and the command line make the same record, roles and invitations
work, and the first owner is set once. One check moves: "projects with colliding slugs" waits for slugs, which arrive
with publishing. Still to come from the P1 list: slugs, locales, environments beyond production, and a generated API
description.

## What it doesn't do yet

- There's only ever one administrator. Making more needs a way for an administrator to promote someone, which comes
  with project administration.
- Anyone with the database owner's password can still change anything. "Permanent" means no normal path undoes it.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**P2, publish exactly once:** approving the exact change rather than just the article, and one release, one number.
