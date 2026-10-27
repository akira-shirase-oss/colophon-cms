+++
title = "Checkpoint 2.3: work that survives a crash"
description = "Colophon now tells the outside world when it publishes. A background worker announces every release to your webhooks, keeps trying when they're down, gives up cleanly when they refuse, skips an older release once a newer one is queued, survives being killed halfway through, and only ever calls public addresses."
date = 2026-10-26T13:02:06Z

[extra]
kind = "release"
milestone = "P2"
step = "2.3"
short_title = "Work that survives a crash"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 210

[extra.summary]
goal = "Announce every release to the outside world, even if the worker dies halfway, without ever pretending a duplicate is impossible."
built = "A transactional outbox, a lease-based worker with retries and supersession, webhook destinations restricted to public addresses, and crash test F03."
proof = ["killed after sending: resent with the same key", "an older release is skipped once a newer one is queued", "1,000,201 publishing requests, 0 failed"]
code = "akira-shirase-oss/colophon-cms · main @ 070415e"
+++

Publishing is only half the job. Usually something else has to hear about it: a site to rebuild, a cache to clear, a
chat message to send. That something lives outside Colophon's database, and that's where crashes get interesting.

## What I built

- **Destinations.** A project can add webhooks: addresses Colophon calls whenever a release goes live.
- **An outbox.** Publishing writes "announce release 5 to the site rebuild" into the database in the same step as
  the release itself. If the release happened, the announcement is queued; if it didn't, it isn't. Nothing can fall
  between the two.
- **A worker.** A background process picks queued announcements up and sends them. When a destination is down it
  tries again later, waiting longer each time, up to five tries in an hour. When a destination refuses outright, it
  stops and says why.
- **Never out of order.** If release 6 is published before release 5's announcement went out, release 5's is
  skipped and marked as such. One gap, found a step later: if a second worker picked up release 6 while release
  5 was still on its way, a slow network could deliver 5 last. The next step's simulator caught it, and 2.4 fixes
  it.
- **The Releases page shows deliveries**, release by release: delivered, retrying, failed, or skipped.

## How it works (the short version)

The hard case is a crash right after the worker sends an announcement but before it writes down that it did. It can't
know whether the other side got it. So each job is held under a lease: if the worker vanishes, the lease runs out,
another worker picks the job up and sends it again, with the same unique key it used the first time. A receiver that
checks that key can drop the repeat. Colophon doesn't claim duplicates are impossible; it hands the receiver what it
needs to ignore one.

Leases also stop a slow, forgotten worker from overwriting a newer result: an attempt can only record its outcome if
it still holds the lease it started with.

Webhook addresses come from people, so they're treated with suspicion. A webhook could point at the server's own
internal network, its database or the cloud's private metadata service. The worker only connects to public
addresses. It checks the address again at the moment of connecting, so a name can't be switched to an internal
address later, and it never follows redirects.

## Does it actually work?

- **Killed mid-delivery (crash test F03):** the worker sends an announcement and is killed before recording it. A
  fresh worker picks the job up once the lease runs out, sends it again with the identical key, and records success
  after two attempts.
- **Out of order (F05):** two releases are published while no worker is running. When one starts, it sends release
  2 and marks release 1 skipped; the receiver only ever sees release 2.
- **Down, then up:** a destination answers "unavailable" once; the second try succeeds, with the same key.
  A destination that refuses gets one try and a clear failure.
- **Leases in the database itself:** a held job is invisible to other workers until its lease runs out; a late
  result from the old lease is refused; two workers grabbing jobs at the same instant never get the same one.
- **The worker's database account** can see delivery jobs across projects, and is refused drafts, members and
  sessions.
- **In real browsers:** add a webhook on the Releases page, publish, and watch it turn "delivered".
- **A million publishing requests:** 1,000,201 requests at 680 a second (200,000 whole publishes, each five requests
  that all write), with the worker announcing every release to a local receiver, none failed (p99 387 ms). At a
  steady 40 publishes a second, p99 19 ms.
- **210 Rust tests,** 16 Studio tests against the server, 18 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40327179132)).

## Where I messed up

- **The worker napped between every batch.** It took a handful of jobs, then slept half a second, whether or not
  more were waiting, so its speed was capped by its own alarm clock. The load test made it obvious. Now it only sleeps
  when there's nothing to do.
- **The worker shared the editors' database connections.** My first steady load test, a hundred publishes a second
  across 512 projects, had the worker delivering about a hundred announcements a second through the same small pool
  of database connections that editors' requests use, and editors waited behind it. The module's own design says
  background work must never starve editorial saves; now the worker has its own pool. That test still fell behind:
  every request in it writes, and this laptop tops out around 500 to 700 writes a second, so I kept the failed
  result and set the steady rate to 40 publishes a second.
- **A million URLs, a million counters.** In the new load test every request has its own address, and the load tool
  started tracking each one separately; at a million requests that would have run it out of memory. Requests are now
  grouped by step.
- **My load-test baseline kept failing for reasons outside the code.** The million-request run above missed the p99
  limit I'd first picked (250 ms, a guess); it's within the 700 ms the project budgets for writes, which I now use. A
  later rerun was ruined when my laptop went to sleep in the middle, and the one after that got cut off at 874,279
  requests by the test's time limit, with 82 requests turned away (0.01%), on a database grown to several gigabytes
  by a day of load tests. Every one of those results is kept.
- **CI failed twice for reasons that weren't in the step.** Once because Linux handed an outgoing connection the very
  port a test server was about to use (the stack's ports sat inside the range Linux uses for outgoing connections;
  CI now keeps them apart). Once because a test from step 1.6 stopped reading the command line's output too early, and
  the CLI crashed writing its next line. Both fixed.
- **A test that signed in before the server existed.** The crash test logged in first and started the server second.
  Signing in goes through the server, so it failed at once. Easy to fix, and it should have been obvious.

## What it doesn't do yet

- Announcements aren't signed yet, so a receiver can't prove they came from Colophon. Signing comes with destination
  secrets.
- Receivers have to check the key themselves to ignore a repeat.
- Only webhooks so far. Cache purges and site builds come with the next milestone.
- The worker checks for work by polling rather than being woken up.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**2.4, killing it on purpose:** teaching the simulator about releases and the outbox, and a loop that really kills
the server and worker, over and over, and checks nothing was lost or doubled.
