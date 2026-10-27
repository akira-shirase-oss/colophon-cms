+++
title = "Checkpoint 0.3: five million requests, and the bug they didn't find"
description = "Colophon learned to say when something's wrong: one error format, honest health checks, a separate frontend that tests it, and load tests in the millions. The millions found nothing. The frontend found a bug on day one."
date = 2026-10-25T19:22:40Z

[extra]
kind = "experiment"
milestone = "P0"
step = "0.3"
short_title = "Five million requests, and the bug they didn't find"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 63

[extra.summary]
goal = "A server that tells the truth about its own health, and the habit of proving every feature three ways."
built = "One error format, request IDs, /livez, /readyz and colophon doctor; a separate testbed frontend; a load-test harness that keeps its results."
proof = ["63 Rust tests, 6 browser tests", "5,000,000 requests, 0 failed", "a real bug found and fixed, with tests that failed first"]
code = "akira-shirase-oss/colophon-cms · main @ 8d0f80f"
+++

A smoke alarm that beeps "all fine" without ever checking for smoke is worse than no alarm at all.

This step was about making Colophon's server check, and say, when something is wrong.

## A new rule

Before writing any code, I set a rule for the whole project. Every feature now ships with three kinds of proof:

- **Regression tests:** automatic tests that run on every change, so a feature that works today can't quietly break
  tomorrow.
- **A separate frontend to test it:** a small website, the *testbed*, that uses Colophon exactly the way a real app
  would. A robot (Playwright) clicks through it on every change.
- **Load tests:** throw millions of requests at it and see what breaks.

## What I built

- **One error format.** Whatever goes wrong, a wrong address, a file that's too big, a crash, the answer has the same
  shape: a code a program can check, a sentence a person can read, and a **request ID**. That's a unique tag on every
  request, so "it failed" plus one string is enough to find exactly what happened in the logs.
- **Two health checks.** `/livez` answers "is the process alive?". `/readyz` answers "should anyone send me traffic right
  now?", and it can say *ready*, *degraded* (the cache is down, so things are slower, but nothing is unsafe) or
  *not ready* (the database is down).
- **`colophon doctor`,** a command that asks a running server how it's doing and explains the answer.
- **The testbed,** with three pages: live health, the API's rules checked from a browser, and every load-test result.
- **A load-test harness** that runs a fresh server, fires the requests, and saves the results with the machine and
  version they came from.

## How it works (the short version)

The obvious way to build `/readyz` is to check the database every time someone asks. Then a flood of health checks
becomes a flood of database queries, and the health check itself takes the system down.

So Colophon checks its database and cache in the background, every two seconds. `/readyz` just reads the latest answer.
Asking a million times costs the database nothing extra.

If the background checker itself ever crashed, readiness would freeze at its last answer, maybe "all good", forever.
So something watches the checker too, and marks the service as down if it ever stops.

## Does it actually work?

- **5,000,000 requests** in 66 seconds (75,405 a second), **0 failed**. Every single answer was checked: the right
  status, the right content and a request ID, 15 million checks in all. 99% of the answers took less than 1.7
  milliseconds.
- **The database saw 13 queries during those five million requests.** All of them came from the background checker.
- **A steady 20,000 requests a second for a full minute:** 1.2 million requests, 0 failed, 99% answered in a quarter of
  a millisecond.
- **Real outages, on purpose:** the tests stop the real cache. Colophon says *degraded* and keeps serving. They stop
  the real database: it says *not ready* within a few seconds, and recovers on its own when the database returns.
- **Shutting down politely:** when told to stop, the server first says "draining" so traffic moves elsewhere, finishes
  what it's doing, then exits.
- All of it runs on every change: **63 Rust tests and 6 browser tests,** in CI as well as on my laptop. The first CI run
  passed everything, including the outage tests and a 200,000-request load test on GitHub's 4-core machine (18,912 a
  second, 0 failed) ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40229252959)).

These numbers come from my laptop (an M2 Pro), with the load tester running on the same machine. They say the code is
fast and correct; they don't say what a production server will do.

## The bug the millions didn't find

Five million requests found nothing. The testbed found a real bug on its first run.

One of its checks uploads a file that's too big, expecting a polite "too large" answer. Instead it got a "bad gateway"
error. Sent straight to the server, the same upload got the right answer. So I tried other clients. Chrome: fine. curl:
fine. Node.js's built-in `fetch`: **4 failures out of 10.**

My first theory was wrong: the test I wrote for it passed against the buggy server. The second theory was right, and the
test for it failed the way it should.

Here's what was happening. When the server refused an upload without reading it, it hung up the connection afterwards,
**without saying so**. Browsers and curl noticed and quietly reconnected. Clients that keep connections open for reuse
(most of them, for speed) sent their next request down a dead line.

The fix is what the HTTP rules recommend: read and discard the rest of the upload first (within limits, 32 MB and two
seconds), or, if that's not possible, say "I'm closing this connection" out loud. The test for it failed before the
fix and passes after it. The same Node.js check now gets the right answer 10 times out of 10.

## Where I messed up

- **My load test failed its own rule.** The steady test has to send exactly 20,000 requests a second, and the first run
  missed 271 of its 1.2 million. The server was fine; the load tester couldn't keep up because it was starting its
  workers too late. I fixed that, and wrote down a limit (at most 0.01% missed) instead of tuning until it passed. Both
  failed runs are still in the results, on purpose.
- **A test with a lying name.** "Fails fast when the database is down" took two full seconds, because the database
  library quietly retries. The test is now named for what it actually proves.
- **A 200-second test.** After the fix, connections stayed open (which is the point), and one test sat waiting for a
  hang-up that never came. It now reads exactly one answer and moves on: 0.4 seconds.
- **We nearly built the frontend from memory.** SvelteKit 3 came out on 1 October, newer than anything Claude had
  learned from. We read the migration guide and started from the official template instead, which was wise: almost
  everything we'd have typed from memory had changed.

## What it doesn't do yet

Colophon still can't store a single article. It can now say, honestly and quickly, whether it's in a fit state to.

## Up next

**0.4, the editor spike:** the writing editor, and the promise that what goes in comes back out exactly the same.
