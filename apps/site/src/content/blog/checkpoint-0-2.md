+++
title = "Checkpoint 0.2: healthy is not the same as working"
description = "Colophon's local stack: a database, a cache, file storage and sign-in, all started with one command. Docker said every service was healthy, so I broke one on purpose to see whether anyone would notice."
date = 2026-10-25T18:20:04Z

[extra]
# experiment, failure or release
kind = "experiment"
milestone = "P0"
step = "0.2"
short_title = "Healthy is not the same as working"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 6

[extra.summary]
goal = "Everything the server will need, running on my laptop with one command, and a way to know it's really working."
built = "A local stack of four services (PostgreSQL, Valkey, Garage, Keycloak) and a script that checks each one from the outside, 28 checks in all."
proof = ["28 checks pass, locally and in CI", "a broken cache that Docker called healthy gets caught", "a real sign-in, done without a browser"]
code = "akira-shirase-oss/colophon-cms · main @ 6994073"
+++

A CMS doesn't run on its own. Colophon needs four other programs next to it: a database, a cache, somewhere to keep
uploaded images, and a service that handles signing in.

This step starts all four with one command. The interesting part turned out to be a different question: how do you
know they're actually working?

## What I built

- **PostgreSQL 18,** the database: the one place where the truth lives.
- **Valkey,** the cache: it makes things fast and is allowed to forget everything. It's a free fork of Redis, whose
  licence changed in 2024.
- **Garage,** file storage that speaks the same language as Amazon S3. Images will live here.
- **Keycloak,** the sign-in service. Colophon will never store passwords itself.
- **One command,** `scripts/stack.sh up`, that starts them, waits, and then runs 28 checks.
- Every service is pinned to an exact image, down to its digest (a fingerprint of the download), and only reachable
  from my own laptop. The test passwords are public, so nothing outside can get to them.

## The file store in the plan was gone

The original spec said to use MinIO for file storage. When I went to install it, its download page on Docker Hub didn't
exist anymore. MinIO stopped publishing free builds in October 2025, and the project has since been archived.

The obvious replacement was RustFS: written in Rust, popular, and built as a drop-in for MinIO. Then I read its security
advisories. In 2026 alone, it had a bug that let people skip the size and type limits on browser uploads, and another
that let users copy files they weren't allowed to read. Both are fixed now, but that's a track record in exactly the two
things Colophon's image uploads will rely on.

So I picked Garage, which is also written in Rust, is boring in the good way, and has been used in production for
years. It's missing a few features (it can't keep old versions of a file), and the decision log says so, along with
when that will matter.

## Healthy is not the same as working

Docker has "healthchecks": every few seconds, it asks each container whether it's fine. When all four said yes, Docker
showed them as **healthy**.

Here's the problem. A healthcheck asks a container about itself, from the inside. It doesn't tell you whether my code
can use it from the outside, or whether it's set up the way I need it.

So the script checks each service the way the Colophon server will, from my laptop through the same port, and every
service has to refuse something:

- **The database** refuses a wrong password, then saves a row and reads it back.
- **The cache** refuses every command until it gets its password, and must be configured never to save to disk and
  never to grow past 128 MB. Something that can't keep anything can't become the source of truth by accident.
- **File storage** saves a file and returns the exact same bytes. It refuses anonymous reads, a wrong key, and an upload
  whose contents don't match its fingerprint (a corrupted upload, basically).
- **Sign-in** does a complete, real login with no browser, using the same steps a browser would. Then it refuses a login
  code used twice, a stolen code without its secret proof (that's what PKCE adds), a wrong password, a login without
  PKCE, a redirect to a page that isn't registered, and the old "just send me the password" shortcut.

## Does it actually work?

- **Everything:** 28 checks pass on my laptop, and the same 28 pass on GitHub's machines from a fresh copy of the code
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40220486515)).
- **Breaking it on purpose:** I told the running cache to start saving to disk and removed its memory limit. Docker
  still said **healthy**. My script said `FAIL memory is capped at 128 MiB: expected '134217728', got '0'` and
  `FAIL never writes an append-only file`. Then I stopped the file store completely, and the script still went on to
  check sign-in instead of giving up at the first failure.
- **Restarting:** I stopped everything and started it again. The database still had the row I'd saved, and everything
  came back in 15 seconds.

## Where I messed up

- **My own healthcheck had a bug.** For Keycloak, I first checked that its health page contained `"status": "UP"`. But
  that page lists several smaller checks, each with its own "UP". If Keycloak as a whole were down while one small part
  was still up, my check would have said all was fine. Now it looks at the page's HTTP status code, which is 200 only
  when everything is up.
- **Two of my "refuses" checks could pass by accident.** "A wrong password gives no login code" is also true when the
  sign-in service is completely broken. Those checks now insist on the exact error message, not just the absence of
  success.
- **One failure hid the rest.** My script stopped at the first error, so a broken file store would have hidden the
  results for sign-in. Now every check runs and reports on its own.
- **A sneaky one:** inside its own container, the database lets anyone in from the same machine with no password at all.
  A password test written the obvious way would have proven nothing. The check now connects the way the server will,
  where a password is required.
- **The usual ports were taken.** Other projects on my laptop already use the default database and cache ports, so
  Colophon has its own set.

## What it doesn't do yet

Colophon itself doesn't use any of this yet. The server can't talk to the database, and nobody can sign in to Colophon.
The stack is ready, and it's honest about whether it's working.

## Up next

**0.3, one error format and honest health checks:** the server gets its first real endpoints, `/livez` and `/readyz`,
and the rule that "ready" must mean ready. If the cache is down, Colophon should say it's running slower, not that it's
broken.
