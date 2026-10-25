# 16 · docs site (`apps/docs`)

> Guides, references, runbooks and the technical write-ups. It is honest about what is implemented and what is only
> planned.

| | |
|---|---|
| Milestones | P3 (skeleton and quickstart), P6 (complete guides; every example runs in CI) |
| Proposed owner | Claude; the maintainer writes the deep-dive articles |
| Depends on | ui; mdsvex, Pagefind |
| Used by | editors, developers, operators, contributors, portfolio readers |
| Spec | §18, §19, §27 |

## Why it exists

An independent install from the README is part of v0.1's definition of done, and documentation that drifts from the
code is worse than none. The deep-dive write-ups are also half of the project's portfolio value.

## End goal

A prerendered, searchable docs site. Every code example runs in CI, links are checked on release, and every page
clearly labels what is implemented vs planned.

## Responsibilities

- Guides, references, runbooks and write-ups.
- Static search.
- Example testing.
- Capability labels.

## Not its job

- Product marketing claims. There are no uncited speed claims and no "secure by default" promise (spec §18).

## Breakdown

- **Stack:** mdsvex + Pagefind, prerendered, using the same design system as the site with its own navigation.
- **Content:**
  - editor onboarding and a developer quickstart;
  - schema examples;
  - CLI, API and SDK reference;
  - self-hosting, backup and restore, upgrades;
  - troubleshooting, security reporting, known limits.
- **Deep dives:**
  - the outbox;
  - release generations and epochs;
  - the deterministic simulator;
  - cache invalidation;
  - delivery receipts.
- **Versioning:** product docs are versioned (spec §19).
- **CI:** code examples and template installs run in CI, and links are checked.

## Invariants

| ID | Invariant |
|---|---|
| DS1 | No page describes a planned feature as available. |
| DS2 | Every code example on a released version runs in CI. |

## Interfaces

- Static site output, plus a Pagefind index.

## Failure modes

| Situation | Behaviour |
|---|---|
| An example breaks after a change | CI fails before the release |
| A link goes dead | The release link check fails |

## Tests and evidence

- Example execution and link checks in CI.
- The independent-install trial uses only the docs, and its friction is written up.

## Build order

1. **P3:** skeleton and quickstart.
2. **P6:** complete guides, references, runbooks and the deep dives.

## Open decisions

- Where docs are hosted, and whether the docs site is itself published through Colophon later.
