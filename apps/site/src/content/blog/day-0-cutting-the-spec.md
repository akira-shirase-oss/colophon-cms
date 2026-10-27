+++
title = "Day 0: a 41-section spec, cut down to four promises"
description = "I started with a complete specification for a CMS, with a shop, subscriptions and AI included. The first job was deciding what not to build."
date = 2026-10-25T09:00:00Z

[extra]
# experiment, failure or release
kind = "experiment"
milestone = "D"
step = "D.1"
short_title = "A 41-section spec, cut to four promises"

[extra.summary]
goal = "Understand the whole spec, then decide what this project is actually for."
built = "An honest review of the spec, a new goal, four promises the product has to keep, and a list of what's parked."
proof = ["41 sections read, 16,827 words", "the shop, payments and subscriptions parked", "0 lines of code"]
+++

Have you ever clicked "Publish", refreshed your website, and still seen the old version sitting there?

Did it publish? Is it stuck? Should you click again? Nobody knows. Most content tools show a little spinner and hope for the best.

That small moment is where this project starts.

## What I built

No code yet. Today was reading and deciding.

I had a complete specification for a content management system, or CMS: the app where you write, review and publish a website's articles. It runs to 41 sections and 16,827 words. It covers editors, developers, AI agents, caching, payments, subscriptions, shipping, tax and refunds, plus the tests to prove all of it.

I asked Claude Code to read the whole thing and tell me honestly what it thought.

## How it works (the short version)

The honest answer: great engineering, impossible scope.

- **The good part:** the spec takes the hard stuff seriously. Saves that can't silently overwrite each other. Publishing as one numbered step. Background work that survives crashes. Never saying "done" without proof.
- **The problem:** it's three products in one: a CMS, an online shop and a subscription business. Each of those alone is a big project.
- **The trap:** in 2026, a giant repo with dozens of features can be generated in a day, and people know it. A long feature list now looks like exactly the AI hype I want to avoid.

So I changed the goal. This is an open-source project and a portfolio piece. It doesn't need to sell. It needs to be genuinely useful, and **provably** correct.

That turned into four promises:

1. **Kill it at any moment, lose nothing.** Pull the plug in the middle of a publish: no saved edit disappears, nothing publishes twice, and no reader sees half a release.
2. **Every bug is a number you can replay.** The core logic runs inside a simulator with a fake clock and a fake, unreliable network, thousands of times on every change. A failure comes with a seed number that reproduces it exactly. (Databases like FoundationDB and TigerBeetle are known for this. Almost no website software does it.)
3. **Time travel.** Every publish becomes a numbered release. You can look at the whole site as it was at any release, and "rolling back" is just a new release with older content.
4. **Receipts, not spinners.** After you publish, the screen says *published in the CMS → website update pending → seen live at this location*, or it tells you exactly what went wrong.

Everything else got sorted into *core*, *later* or *parked*. The shop, payments, subscriptions, shipping and tax are parked, along with hosting integrations and custom domains. They're good ideas. They're just breadth, not proof.

AI got demoted too. Scripts and AI agents still get access, but limited and safe: they can draft and propose, and they can't approve their own work. It's a useful feature, not the headline.

## Does it actually work?

There's nothing to run yet, so: no.

What I can check is the paper trail. Every decision is written down with its reason, so I can't quietly drift: 12 made so far, and 16 still open, each with the milestone where it has to be settled.

## Where I messed up

My first time estimate was wrong in a useful direction. I planned around 1–2 hours a day, which put even a basic version more than a year away. Then I looked at it again: I can give this 8 to 16 hours a day. Planning on 8, version 0.1 comes out at roughly 4 to 6 months.

Some of it won't go faster with more hours, though. Testing with real editors, a second person reviewing the risky code, and a security review all need other people and calendar time.

## What it doesn't do yet

Anything. It's a plan.

## Up next

Giving it a name, and writing the plan down properly before the first line of code.
