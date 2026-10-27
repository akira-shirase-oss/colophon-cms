+++
title = "Checkpoint 2.4: killing it on purpose"
description = "A simulator now races publishers, crashes workers and scrambles the network thousands of times a run, and a crash loop really kills the server over and over while it publishes. Building the simulator found a real ordering bug in the previous step; it's fixed, and neither the simulator nor the crash loop finds anything lost or doubled. The second milestone is done."
date = 2026-10-26T16:19:07Z

[extra]
kind = "release"
milestone = "P2"
step = "2.4"
short_title = "Killing it on purpose"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 217

[extra.summary]
goal = "Attack publishing and delivery as hard as possible, in a simulator and with real crashes, and fix whatever breaks."
built = "A publishing simulator with nine rules and eight planted bugs, a real kill -9 crash loop, and one announcement in flight per destination."
proof = ["all eight planted bugs caught", "20,000 simulated runs, no rule broken", "12 kills, 524 releases, nothing lost"]
code = "akira-shirase-oss/colophon-cms · main @ c38ec47"
+++

The last two steps made claims: a release takes exactly one number, an announcement survives a crash, an older release
is never sent after a newer one. Tests check the cases I thought of. This step goes looking for the cases I didn't.

## What I built

- **A publishing simulator.** It plays publishers racing each other (some working from an out-of-date view), workers
  that crash at random moments, and a network that loses requests, loses answers, delivers things twice and gets
  refused, all on a fake clock, with every random choice coming from one number. The same number always replays the
  same run, step for step.
- **Rules checked after every step:** two releases never share a starting number; a release is never applied twice;
  a retried publish that went through is told so; numbers go up by one; an entry is never live as two versions at
  once; every release queues its announcements; no destination hears an older release after a newer one; only the
  worker holding a job can record its result. At the end, after a calm period, every announcement must have finished.
- **Proof the simulator has teeth.** Eight bugs planted on purpose, each one a rule broken in a believable way. The
  test suite requires every one to be caught.
- **A real crash loop.** A client publishes in two projects while the actual server and worker are killed with
  `kill -9` at random moments, twelve times, and restarted; whatever got no answer is sent again. Then a checker
  compares what the client was told with the database, the public API and what the webhook received.

## How it works (the short version)

The simulator uses the same rules as the real system, straight from the same code: how a release gets its number,
how publication history is updated, when a failed delivery is retried. It replaces only the database, the network
and the clock with fakes it controls, so it can explore thousands of interleavings a minute, and when something
breaks, print the one number that reproduces it.

The crash loop is the honest counterpart: real processes, a real database, a real network, real kills. It can't
explore as much, but it can't be fooled by a model that's subtly wrong either.

## Does it actually work?

- **200 simulated runs in the test suite, 1,000 more in CI, and one sweep of 20,000,** with no rule broken: 4.4
  million releases, 139,000 worker crashes, 475,000 expired leases taken over, and every other fault actually
  happening.
- **All eight planted bugs caught,** five on the very first seed; the three about delivery order (two workers sending
  to one destination at once, skipping the check for a newer release, and sending with too little of a claim left)
  on the 8th, the 67th and the 61st.
- **The crash loop:** 12 kills, 524 releases acknowledged and checked, 9 requests cut off mid-flight and sent again.
  Every acknowledged release exists with the number the client was told, numbers run 1 to N with no gaps, no
  history overlaps, every announcement arrived in order, and the public API shows exactly the right entries at
  past releases.
- **217 Rust tests,** 16 Studio tests against the server, 18 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40352901560)).

## Where I messed up

- **The previous step had an ordering bug, and the simulator found it.** While modelling the worker, I had to decide
  what happens if release 5 is published while release 4 is still being sent. In the real system, a second worker
  could pick up release 5 and send it straight away, and if the network was slow, release 4 could arrive *after*
  release 5, exactly what step 2.3 promised couldn't happen. Its tests never covered that timing. Now a destination
  only ever has one announcement in flight, and a worker gives up rather than send when its claim is about to run out.
  The simulator proves it: remove the one-at-a-time rule and it catches the bug.
- **The simulator's first bug was its own.** The very first run failed a rule, but because my model let a release
  pick the same article twice, which the real system never allows. A simulator is code too.
- **And so was its second, which hid a bigger problem.** The first 1,000-run sweep failed at run 449: a release
  arrived after a newer one. The cause was the model again: a worker checked how much time it had left against the
  job's *current* claim, which by then belonged to another worker, so a worker whose claim had run out thought it had
  plenty of time and sent anyway. The real worker measures its own claim. Fixing the model then showed something
  worse: the two delivery-order bugs I had planted were now never caught. Their earlier catches had come through the
  model's own bug. In the model, a request was in flight for under a second while a second worker needed about three
  turns to send, the opposite of real life, so two sends almost never crossed. I made in-flight time long compared
  with how fast workers act, and planted an eighth bug that removes the time-left check itself. All eight are caught
  now. I had pushed the 1,000-run sweep to CI after running only 200 runs on my machine; CI caught it.
- **The model's bug pointed at a small real one.** The worker started timing its claim after the database answered,
  but the database starts the claim while it runs the query. On a slow database, the worker could think it had more
  time than it did. It now starts the clock before asking, so it can only think it has less.
- **A list four times slower, and the code hadn't changed.** CI's load test of changesets failed twice on this step:
  listing them averaged 225 to 378 ms where step 2.3 had 104, on the same machines, while every other endpoint was
  as fast as before. The query hadn't changed since 2.2, and I couldn't reproduce it on my laptop. So I had CI record
  the plans of slow queries. The database keeps each prepared query's plan, and it had settled on one while the
  table of reviews was nearly empty: read the whole table once for each of the 100 changesets on the page. Then the
  test grew that table to 3,000 rows in 40 seconds, and the plan stayed. The previous step had simply been luckier.
  The list is now planned fresh every time, and a test checks it never runs on a kept plan (without the fix, it did
  3 times out of 8).
- **Then it failed again, and this time the query was innocent.** With fresh plans the list still averaged 175 ms on
  some CI runs and 64 ms on others. The recorded plans were good ones, about 2 ms on my laptop, just slow on a
  starved machine. The real difference was the machine: GitHub hands this job different processors, and the same
  code did 1,916 requests a second on one and 783 on another (the health check, which never touches the database,
  did 44,243 and 18,613). The load test ran 64 clients that each wait for an answer before asking again, and with a
  fixed number of waiting clients, the average wait is the number of clients divided by the requests a second the
  machine manages (Little's law). So my "p99 under 200 ms" check was measuring the CI machine. Step 2.3's code
  failed it too, on a slow machine. CI now checks database-backed endpoints at a fixed rate, about half what the
  slowest machine I've seen can do, against the same limits. On the slowest machine yet, every p99 was 5 to 38 ms.
  A slower query or a loss of capacity still fails it.
- **I published a promise the next step broke.** Step 2.3's write-up said an older release is never sent after a
  newer one. This step found the timing gap above, so I corrected that page instead of leaving it.
- **One planted bug slipped through at first.** "A retried publish forgets it already went through" broke nothing
  the simulator checked, because the system refuses the retry instead of applying it twice. But that refusal is
  still a bug: the person is told their publish failed when it didn't. I added a rule for exactly that.
- **The crash loop's first failure was the checker's.** It compared the releases against an API that lists only the
  newest 100, and the run had made 276. It reads the database now.

## What it doesn't do yet

- A process frozen (not killed) for longer than its claim could still send late; receivers should go by the release
  number Colophon sends with every announcement.
- The crash loop kills the server and worker together; killing them separately, and running a fake CDN, come with
  the next milestone.
- The simulator models the database and the network; it doesn't run them. That's what the crash loop is for.
- Other queries could fall into the same kept-plan trap on a fresh install; only the one that did is fixed so far.
- CI's load checks now measure latency at a fixed rate. How much a machine can take is measured by the full
  million-request runs on my laptop, and those numbers are only about that laptop.

## Up next

**3.1, a cache that can't serve the wrong thing:** the first step of P3, where every publish has to be proven to
have reached the site.
