# Why Colophon exists

## The goal, in the maintainer's words

On 2026-10-25 the maintainer set the goal: *"build something crazy for open source and personal portfolio, and
something which is not just another AI jargon … but something more useful."* Selling it is "just an idea", not a goal.

Everything in these docs follows from that sentence. When a scope question comes up, ask: *does this make the project
more provably correct, more genuinely useful, or easier to verify?* If not, it waits.

## What "crazy" means here

Not breadth. In 2026 a repository with dozens of features, five services and an AI panel takes a day to generate, and
people who review portfolios know it. A wide feature list now reads as exactly the hype this project wants to avoid.

Three things are hard to fake, so Colophon is built around them:

1. **Behaviour under failure.** Kill the server in the middle of a publish; show that nothing was lost, duplicated or
   half-published, and prove it with a checker rather than a claim.
2. **Numbers anyone can reproduce.** Benchmarks ship with the exact command, dataset, environment and raw results.
3. **A design the maintainer can defend line by line.** Every invariant has a reason, a test and an owner who
   understands it.

## Why a CMS

- **It is useful every day.** Every developer has a site. The maintainer's own site will run on Colophon, which is the
  first and most honest test of usefulness.
- **It contains nearly every hard backend problem in one coherent product:** concurrent edits, immutable history,
  database transactions, durable background work, caching and invalidation, authorization for people and machines,
  safe uploads, time zones, backups and recovery.
- **Its failures are visible to non-engineers.** "I published it, so why isn't the site updated?" is a real, common
  frustration. Solving it properly turns deep engineering into something an editor can see.
- **The bar is high and honest.** Strapi, Payload and Sanity already document releases, versions and CDN delivery. The
  spec refuses to pretend otherwise (spec §01, §37). Colophon's angle is not missing features; it is behaviour that can
  be proven under failure.

## Why Rust and Svelte

- **Rust** suits a project whose identity is invariants: types that make invalid states hard to build, exhaustive
  matching over state machines, ownership that prevents data races, and a single async runtime (Tokio). It is also the
  language the maintainer is building depth in. Rust is not the moat, and the docs never claim it is.
- **Svelte/SvelteKit** gives one frontend toolchain for three different jobs: a client-side Studio, a prerendered public
  site, and a prerendered docs site.
- **PostgreSQL** is the single source of truth because correctness has to live in transactions and constraints, not
  in application prechecks that race.

## Why not "AI-powered"

AI is deliberately demoted. The spec's `ai` crate becomes **`automation`**: scoped tokens, a CLI with stable JSON
output, and an MCP adapter so scripts and agents can do useful work without being able to break anything. The AI
writing assistant is optional, off by default, and built last. Nothing in the headline mentions AI.

## Why the name

A book's colophon records how, when and by whom it was printed: literally *proof of what was published*. Web
designers already use `/colophon` for the page that explains how a site was built. Colophon's reference site will have a
`/colophon` page showing the live release generation and its delivery receipts, so the name is also a feature.

## What this project is not optimizing for

- Revenue, sign-ups, or market share.
- Feature count, or matching another CMS feature for feature.
- Being first, or the fastest in a synthetic benchmark.

What it does require: **one real user** (the maintainer's site) and **one independent install from the README**.
Without those, "useful" is only a claim.

## Capacity and pace

The maintainer said the project can get 8–16 hours a day. Planning assumes about 8 hours a day (~45 hours a week) and
treats more as occasional pushes, because some gates depend on other people or calendar time (usability tests,
independent installs, a second reviewer, a security review), not hours. See [ROADMAP.md](ROADMAP.md).

## Origin

Colophon derives from the *Complete Product + Engineering Specification: Rust + Svelte CMS* (22 October 2026), kept in
[spec/](spec/README.md). The decisions that narrowed and reframed it, made on 2026-10-25, are listed in
[DECISIONS.md](DECISIONS.md).
