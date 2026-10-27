+++
title = "Checkpoint 1.1: signing in, and projects that can't see each other"
description = "Colophon now knows who is asking: sign-in through a real identity provider, sessions only the server can end, and projects that the database itself hides from everyone but their members. Plus the first load test that has to talk to the database on every request."
date = 2026-10-25T22:29:25Z

[extra]
kind = "release"
milestone = "P1"
step = "1.1"
short_title = "Signing in"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 111

[extra.summary]
goal = "Know who is asking, and make sure one project's drafts can never show up in another project."
built = "Sign-in through an OpenID Connect provider, server-side sessions with CSRF protection, and projects isolated by PostgreSQL's row-level security."
proof = ["14 forged or broken sign-in tokens refused", "a second person sees none of your projects", "1,000,005 signed-in requests, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 397cb59"
+++

Until now, anyone could call Colophon and it would answer. That was fine while all it did was check rich text. A CMS
holds unpublished drafts, though, and the first thing milestone P1 needs is to know who is asking, and to make sure the
answer can never leak one project's drafts into another.

## What I built

- **Sign-in through a real identity provider.** Colophon never sees your password. You sign in at Keycloak (locally;
  any standard OpenID Connect provider later), and it sends you back with signed proof of who you are.
- **Sessions only the server can end.** After sign-in your browser holds a random token, and the database stores only
  a fingerprint of it (a SHA-256 hash), so a stolen copy of the database contains no usable sessions. Signing out ends
  the session on the very next request. Sessions also end after 12 hours, or 2 hours unused.
- **Protection against other websites acting as you.** Every change needs a second secret that only Colophon's own
  pages can read, and has to come from an approved web address.
- **Projects that can't see each other.** Only a project's members can list it or open it. Anyone else asking for it
  gets "not found", the same answer as for a project that doesn't exist, so nobody can probe for names.
- **A Session page in the testbed,** and a load test where every request is signed in.

## How it works (the short version)

Signing in is a round trip. Colophon sends you to the identity provider with three one-time values, and when you come
back it checks the signed token the provider hands over: was it signed with the provider's real key, is it meant for
Colophon, is it fresh, and does it carry the one-time value Colophon sent? Any "no" and you're not signed in.

I wrote that check myself, on top of two widely used libraries, instead of using a ready-made OpenID Connect crate. The
crate depends on an RSA library with a known timing weakness that has no fix yet. Writing the check myself means I
also have to test it against every attack I know of (below).

Keeping projects apart isn't left to the API code. It's enforced by PostgreSQL. The server logs in as a database user
that can't see any project at all unless the transaction says who is asking, and then only that person's projects.
A query that forgets a filter still can't return someone else's data.

## Does it actually work?

- **14 forged or broken tokens are refused,** with signing keys generated fresh for each run: a token signed by a
  different key pretending to be the provider, a wrong or missing one-time value, the wrong issuer, a token meant for
  someone else, expired, not valid yet, no subject, a missing or unknown key, garbage, and two classic tricks: "signing"
  with the provider's public key as if it were a password, and a token that claims it needs no signature at all.
- **Real sign-in, end to end.** The server's tests fill in Keycloak's actual login form against the real server binary,
  then check that changes without the second secret, or from an unapproved address, are refused, and that signing out
  means 401 afterwards. A sign-in link works once, and only in the browser that started it.
- **Two people in two browsers.** In the testbed, one person creates a project; the other doesn't see it in their list,
  and gets 404 opening it directly.
- **Even a careless query is safe.** Database tests run as the application's own user and query each table with no
  filter at all: they still see nothing from projects they don't belong to.
- **A million signed-in requests:** 1,000,005 at 2,671 a second, each one looking its session up in Postgres and
  reading through row-level security, and none failed (p99 30 ms). At a steady 2,000 a second the p99 was 12 ms.
- **111 Rust tests,** plus the testbed in a real browser, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40248086994)).

## Where I messed up

- **The first load test was slow, and Postgres wasn't the problem.** 1,485 requests a second, with the database using
  one of its twelve cores. Each request made more than ten round trips to it: a check that the connection was still
  alive before every query, a write recording "session last used" that changed nothing almost every time, and two
  separate trips for one page. Now the "last used" write happens at most once a minute, the liveness check is gone
  (the library still checks each connection when it's handed back), and the page reads everything in one go. Same
  laptop, same code path: 2,671 requests a second.
- **A full connection pool looked like a crash.** When every database connection was busy, the server answered
  "internal error" and logged it as a bug. My own design doc promised "busy, try again in a second" (503 with
  `Retry-After`). It does that now, and a test holds it to that.
- **The latency limit was wrong for this test.** The 25 ms limit came from the health checks, which never touch the
  database. With 64 simulated users sharing 16 connections, just waiting in line takes about 24 ms. Signed-in requests
  now have their own limit of 100 ms, still far below the 700 ms the project's goals allow for editing.
- **Some dates on this build log were wrong.** Checkpoints 0.4 to 0.7 said 26 October, a day that hadn't happened yet.
  They now show when they were actually published.

## What it doesn't do yet

- Roles beyond "owner", invitations, the command that creates the very first owner, and the one policy function every
  request will go through. Those are next in P1.
- No per-person rate limits yet.
- Only one identity provider has been tested: Keycloak.
- The rich-text check still answers without sign-in.
- These numbers come from one laptop running the database in Docker. They show the code holds up; they aren't a
  promise about production hardware.

## Up next

**1.2, a draft that can't be lost:** entries, revisions, and a save that either fully succeeds or clearly fails, even if
the server is killed halfway through.
