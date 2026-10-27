+++
title = "Checkpoint 3.1: a cache that can't serve the wrong thing"
description = "Colophon's public API now has a cache, built so it can't hand out an old release by mistake: every answer is filed under the exact release it came from, the current release is always checked against the database, a thousand requests for the same thing cause one database read, and if the cache dies, answers keep coming straight from the database."
date = 2026-10-26T16:50:42Z

[extra]
kind = "release"
milestone = "P3"
step = "3.1"
short_title = "A cache that can't serve the wrong thing"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 232

[extra.summary]
goal = "Cache public reads without ever serving an older release, without stampeding the database, and without depending on the cache."
built = "A Valkey cache keyed by release, one database read per hot key, a fallback when the cache is down, ETags and CDN cache headers with purge tags."
proof = ["200 requests for a cold key, 1 database read", "Valkey stopped mid-test: every answer still right", "1,000,058 public reads at 6,340/s, 99.99% from the cache"]
code = "akira-shirase-oss/colophon-cms · main @ e5c874a"
+++

The previous milestone made publishing exact: every release has one number, and the public API can read any of
them. Every read went to the database, though. This step adds a cache, and the hard part of a cache isn't speed:
it's never answering with something that's no longer true.

## What I built

- **A cache filed by release.** Every public answer is stored under a name that includes the release it was read
  from: project, environment, release number, and exactly what was asked. Release 5's answers never change, so a
  stored answer can't go stale. Publishing release 6 doesn't need to clear anything: release 6 simply has new names.
- **The current release always comes from the database.** On every request, Colophon asks the database which release
  is live, then looks in the cache for that release. A cache can't make it serve release 5 after release 6 is out,
  because the cache is never asked which release is current.
- **One database read for a crowd.** If a thousand people ask for the same thing the moment it expires, one of them
  reads the database and the rest wait for that answer.
- **The cache is optional.** If Valkey is down or slow (anything over 100 ms), answers come straight from the
  database, and the server reconnects on its own once Valkey is back.
- **Ready for a CDN.** Answers now carry an ETag (so browsers can ask "has this changed?" and get a tiny "no"), cache
  rules (shared caches may keep a live answer 30 seconds and an answer for a specific release 5 minutes), and tags
  that a CDN can purge by. Purging itself comes next.
- **A header that says where each answer came from:** `hit`, `miss`, `coalesced` (waited for someone else's read) or
  `bypass` (straight from the database).

## How it works (the short version)

The name an answer is stored under is built from numbers and fixed-width ids, with the collection name last, so no
text someone puts in a URL can make two different questions share a name. Things that don't change the answer
(unknown query parameters) aren't part of the name, and things anyone could make up (an empty page for a collection
that doesn't exist, a 404, a huge page) are served but never stored, so nobody can fill the cache with junk.

When the cache doesn't have an answer, the first request to ask takes a ticket for that name; others asking for the
same name wait for it. The ticket holder looks in the cache once more (another request may have just filled it),
then reads the database, stores the answer and hands it to everyone waiting, failures included: if the database is
busy, every waiter gets the same "try again in a second".

## Does it actually work?

- **A new release shows at once (fault test F06):** an article is cached, a second article is published, and the
  very next read lists both, under the new release's name. Asking for release 1 still gets the old list, from the
  cache.
- **A crowd causes one read (L04):** 200 requests at once for an entry nobody had read yet: exactly one went to the
  database.
- **The cache dies (F08):** Valkey is stopped mid-test. Every answer is still right, each under a second, including
  a release published while the cache was down. Valkey comes back, and caching resumes on its own.
- **Two projects never share an answer,** and nothing missing or made up is ever stored.
- **A million requests:** 1,000,058 public reads on my laptop at 6,340 a second, the slowest 1% under 25 ms, none
  failed, 99.99% answered from the cache. Without a cache (step 2.2), the same laptop did 2,650 a second.
- **232 Rust tests,** 17 Studio tests against the server, 18 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40358100644)).

## Where I messed up

- **My first cache made the API no faster.** The load test said 2,597 requests a second, against 2,650 with no cache
  at all. Every request still asked the database for the current release, and that question took five round trips:
  start a transaction, switch to the read-only role, set the project, read, finish. A cached answer saved about as
  much as that cost. The question now takes one round trip, through a small database function that has exactly the
  read-only role's rights and nothing more. Same laptop, same test: 6,296 requests a second, and the slowest 1% got
  2.3 times faster.
- **Waiting in line behind a dead cache.** My first version held a lock while connecting to Valkey. If Valkey
  stopped answering without refusing (a network that drops packets), every request would have queued behind a
  100 ms connection attempt, one after another. I caught it writing the outage test. Now only one request tries to
  connect at a time, the rest go straight to the database, and after a failure nobody retries for a second.
- **A test that couldn't fail the right way.** My first cache-name test checked that a collection name with colons
  in it couldn't pose as another field. It would have failed: the cursor position was free text too. Ids are
  fixed-width numbers now, so the collection is the only free text and comes last.
- **Shared build folders lied to me.** I'd pointed several copies of the repository at one build folder to save
  time. The build tool decided a library was up to date because the *other* copy's files hadn't changed, and linked
  the wrong version. I checked that the last published test count wasn't affected (it wasn't), and each copy has its
  own build folder now.

## What it doesn't do yet

- Nothing is purged from a CDN yet, so a shared cache may keep a live answer up to 30 seconds after a release. Purges
  come next, with taking content back down.
- The "one read for a crowd" works within one server process; several processes could each read once.
- Every request still makes one small database read for the current release. That's the price of never trusting the
  cache about it.
- Load numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**3.2, taking it back:** unpublishing, so removed content stops being served even through old release links, and
purges sent to a (fake, for now) CDN.
