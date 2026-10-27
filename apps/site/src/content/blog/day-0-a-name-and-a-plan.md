+++
title = "Day 0, part 2: a name from the back of a book, and the whole plan written down"
description = "Naming the project, choosing a design system, and writing down how every part works and fails before writing any code."
date = 2026-10-25T18:00:00Z

[extra]
# experiment, failure or release
kind = "experiment"
milestone = "D"
step = "D.2"
short_title = "A name from the back of a book"

[extra.summary]
goal = "Give the project a name and a home, and write down how every part will work, fail and be proven."
built = "The name and logo, a cleared-out workspace, 19 module plans, a system design with 10 diagrams, and this build log."
proof = ["31 documents, 32,947 words", "10 diagrams, all checked to render", "0 broken links", "0 lines of code"]
+++

Most books end with a page nobody reads.

It's called the colophon. It says how the book was made: the typeface, the printer, the date. A small, quiet record of what was printed.

That's exactly what this project is for, so that's its name: **Colophon**.

## What I built

- **A name.** I wanted something from books or psychology that's easy to remember. Colophon beat Imprint, Palimpsest and Stet. (Stet, the proofreader's mark for "let it stand", would have been perfect for undoing an edit, but a popular Rust library already uses the name, and so does another small CMS.) Web designers already use a `/colophon` page to say how a site was built, so my site will get one showing the live release and its receipts.
- **A logo.** A "C" cut like a printer's block: hard edges, 45° corners, no curves.
- **A clean desk.** I moved 14 old experiment folders into an archive. Colophon has its own home now, right next to this site's source.
- **A public repository.** The whole plan lives at [akira-shirase-oss/colophon-cms](https://github.com/akira-shirase-oss/colophon-cms), so anyone can check what I said I'd build against what I actually build.
- **The plan, written down.** 31 documents: why the project exists, what "done" means, the architecture, a roadmap and every decision. The core is one detailed plan per part of the system, 19 in all. Each one says what that part does, what must always be true, how it can fail, and how it will be tested.
- **A system design** with 10 diagrams: how a save works, how a publish works, what happens when a background worker dies halfway through, and how restoring a backup avoids doing things twice.

## How it works (the short version)

One rule sits above everything else: **the database is the only source of truth.**

Caches make things fast, but they're allowed to forget everything. Anything that talks to the outside world, like clearing a cache or rebuilding the site, happens only after the change is safely saved, and it leaves evidence behind.

Every publish gets a number that only goes up: release 41, release 42. Old work can never overwrite newer work, because it carries an older number. Time travel comes almost for free: "show me release 37" is just a lookup.

For the editor app I picked Google's **Material Design 3** as the design language: its colours, type, shapes and motion. I'm not using Google's own Material components, because their README says the project is in maintenance mode. Instead, Material's rules become my own design tokens, on top of an accessible component toolkit for Svelte.

## Does it actually work?

There's no code yet, so the plan is the only thing I could test.

- **Links:** a script checked every link between the 31 documents. 0 broken.
- **Diagrams:** I rendered all 10 with the official Mermaid tool. 5 of them failed the first time: in that diagram language a semicolon means "new statement", and my database examples were full of semicolons. Fixed. All 10 render now.
- **Numbers:** every number taken from the original spec was checked against the spec's text.

## Where I messed up

I set out to cut the scope down. Then I wrote 32,947 words of plan, about twice as long as the 16,827-word spec I started with.

In my defence, the spec says *what* to build. The plan says *how each part fails and how I'll prove it doesn't*, and that part is longer. Still, it's funny.

Also funny: the spec tells you to keep one master document, then lists the command-line tool's commands twice, slightly differently. The plan has one list now.

## What it doesn't do yet

Everything. Colophon is a plan with a name and a logo.

## Up next

**P0, the foundation:** pinned tools, an empty workspace that passes its checks, the local stack (a database, a cache, file storage and sign-in), and the first real test: can the editor's output go into Rust and come back out unchanged?
