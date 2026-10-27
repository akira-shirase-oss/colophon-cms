+++
title = "Checkpoint 1.5: content types you define"
description = "Colophon stops assuming every entry is an article. A project defines its own content types (text, rich text, numbers, booleans, choices, dates), the server checks every entry against its type, a type can only change in ways that keep every stored entry valid, and the Studio draws its editor from the type."
date = 2026-10-26T01:23:22Z

[extra]
kind = "release"
milestone = "P1"
step = "1.5"
short_title = "Content types you define"
# Rust tests in the workspace at this checkpoint (drawn as the home page's sparkline).
tests = 160

[extra.summary]
goal = "Let a site describe its own content, and never let a change to that description break what is already stored."
built = "Versioned content types with six field kinds, validation against them on every save, safe-only changes, and a Studio editor drawn from the type."
proof = ["unsafe type changes refused, every time", "articles stored byte for byte as before", "1,000,071 requests, every read matched its save"]
code = "akira-shirase-oss/colophon-cms · main @ 2254117"
+++

Until now, everything in Colophon was an article: a title and a body. Real sites have events with dates, products with
prices, people with roles. This step lets a project describe its own kinds of content, and the hard part isn't the
describing. It's what happens when the description changes after thousands of entries already follow it.

## What I built

- **Content types.** A project can define, say, *Events*: a name, a start time, a kind (talk or workshop), a capacity,
  whether it's online, some notes, a description. Six kinds of field so far: text, rich text, number, yes/no, a choice
  from a list, and a date and time.
- **Every save is checked against its type.** An unknown field, a missing required one, a capacity of zero when the
  minimum is one, a "kind" that isn't on the list: each is refused, and the answer says exactly which field and why.
- **Types change safely, or not at all.** Adding an optional field, allowing longer text, adding a choice: fine.
  Removing a field, or making an optional one required, could make stored entries invalid, so it's refused with
  "needs a migration" (migrations come later).
- **The Studio follows the type.** It draws a text box, a number input, a checkbox, a dropdown, a date picker or a rich
  text editor for each field, and autosave, the device copy and the conflict screen work for any type.
- **Articles became an ordinary type,** every project starts with one, and every article ever saved is still valid,
  byte for byte.

## How it works (the short version)

A content type is a small, strict description in JSON. The server never trusts it: it parses it, refuses anything it
doesn't recognise, and stores it in one canonical form with a fingerprint. Each change becomes a new version, and every
saved entry records which version it was checked against.

Numbers needed care. The server is written in Rust and the Studio in JavaScript, and both must write a price like 19.9
exactly the same way, or the "has anything changed?" checks break. So whole numbers stay within the range JavaScript
handles exactly, and decimals have at most six places. Within those limits, both write identical text.

## Does it actually work?

- **End to end against the real server:** define a type, create entries that fit it, see four kinds of mistake refused
  at the right field, make a safe change and keep using old entries, and see a stale or unsafe change refused.
- **Permissions:** an editor can read types and write entries, but only owners and developers can change a type.
- **In the Studio, in a real browser:** an Events entry with every kind of field, filled in, autosaved, reloaded and
  read back, stored in the right order with the time in UTC, and no accessibility problems.
- **The database keeps the rules too:** versions can't be edited, deleted or skipped, and an entry can't claim a type
  version that doesn't exist.
- **A million requests of typed content:** 1,000,071 at 1,689 a second, a fifth of them saves checked against a
  seven-field type, none failed (p99 60 ms), and every read returned its writer's last save. Checking types isn't
  free: the plain drafts test is about 10% slower than in step 1.2 (1,698 against 1,893 a second), because every
  request now looks its type up.
- **160 Rust tests,** 10 Studio tests against the server, 13 testbed tests, and CI passed every job
  ([CI run](https://github.com/akira-shirase-oss/colophon-cms/actions/runs/40254640887)).

## Where I messed up

- **Numbers came back empty.** The Studio's number field saved nothing: the framework hands number inputs over as
  numbers, and my code only expected text. A browser test that reads every value back after a reload caught it.
- **A migration that would have silently done nothing in production.** Filling in the new "schema version" column for
  existing revisions worked on my machine because the local database user is a superuser and skips row-level security.
  Production's isn't, and the update would have touched zero rows. The migration now switches the rule off for itself
  and back on.
- **Two tests that blocked themselves.** One opened a second database transaction while still holding the only
  connection; another lost its own setup by rolling back after a deliberate error. Both were test bugs, not code
  bugs.

## What it doesn't do yet

- No images, links to other entries, or lists as fields; those need media and references.
- No migrations: a risky change to a type is refused, not carried out.
- Types are defined in JSON (through the API or the testbed); a designer screen in the Studio comes later.
- These numbers come from one laptop. They show the code holds up; they aren't a promise about production hardware.

## Up next

**1.6, the command line:** sign in from a terminal and create the same draft the Studio creates, completing the first
slice the spec asks for.
