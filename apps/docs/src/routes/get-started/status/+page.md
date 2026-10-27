<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon's milestones are each named after what they prove, and a milestone is done when its **proof gate** passes with stored evidence (a commit, a command and its raw output), not when its screens exist. Estimates are guesses at about 45 hours a week, not measurements.

## Milestones

| | Milestone | Proof gate | Where it stands |
|---|---|---|---|
| P0 | Foundation | Editor JSON round-trips; Rust rejects invalid documents; CI is green | Done (October 2026) |
| P1 | Never lose an edit | Kill before and after commit (F01, F02), safe conflicts (F04), cross-project denial, first simulator runs | Done |
| P2 | Publish exactly once | Crashed workers reconcile (F03), stale deliveries are superseded (F05), concurrent releases can't both win | Done |
| P3 | Prove it reached the site | Cache and purge faults (F06 to F08), load cases L01 to L04, the mixed-release checker; first public demo | In progress |
| P4 | Time travel and media | Rollback, pinned reads, revocation, upload fault tests | Planned |
| P5 | Safe automation | Zero unauthorized agent actions; an interrupted bulk job resumes | Planned |
| P6 | Operate it | The rest of the fault and load matrix, a 24-hour soak, a restore drill; **v0.1** | Planned |

The full plan, with what each milestone builds, is the [roadmap](/design/roadmap/).

## Milestone P3, step by step

- **3.1** A cache that can't serve the wrong thing: generation-keyed answers for the public API. <Status implemented />
- **3.2** Taking it back: unpublishing with tombstones, revocation through historical addresses, CDN purge jobs. <Status implemented />
- **3.3** A receipt for every publish: probes, and evidence scoped to the locations checked. <Status implemented />
- **3.4** The reference website, built from one release and promoted atomically. It runs locally; hosting it is the maintainer's decision (O-012). <Status implemented />
- **3.5** The Studio, designed: title search, entry states, version comparison, review differences and the design system these docs use. <Status implemented />
- **3.6** Content modelling in the Studio: a content-type builder, URL names (slugs), and changes checked against stored content. <Status building="step 3.6" />
- **3.7** The maintainer's site goes live, plain words throughout the Studio, one-click publishing, these docs, and the first public demo. <Status planned="P3" />

## Decided since, not yet merged

On 2026-10-27 the maintainer set two further goals, recorded on the step 3.6 branch and not yet in the code these docs were built from:

- **Parity with Strapi and Sanity** (D-057): every feature they offer, built at Colophon's bar of proof, in four new milestones for roles and permissions, content modelling, editorial work and the developer surface. That moves v0.1 to about 9 to 12 months in all, from the 4 to 6 the roadmap above estimates.
- **Non-technical editors first** (D-059): plain words on every screen, one obvious path from writing to publishing, and technical detail only on request. The [editor guide](/editors/) already uses those words.

## What v0.1 means

Version 0.1 is done when every one of these has stored evidence. None is checked yet.

- The maintainer's own site runs on Colophon in production.
- A recorded demo of the crash loop: random `kill -9` of the server and worker during saves and publishes, then a checker proving nothing accepted was lost, no release ran twice and no reader saw a mixed release.
- The deterministic simulator runs in CI on every pull request, any failing seed replays exactly, and a deliberately broken rule is caught.
- Every fault case F01 to F14 and load case L01 to L14 passes.
- A benchmark report on the reference profile, with commands, dataset seed and raw results.
- A backup-and-restore drill onto clean infrastructure.
- One person other than the maintainer installs Colophon from the README alone, and the friction is written up.
- Five editors try draft, preview, review and publish; at least four succeed without help.
- A release with signed artifacts, an SBOM, a changelog, this documentation site, and a technical write-up for each hard problem.

## Following along

Each step is written up, mistakes included, in the build log at [404human.dev/colophon](https://404human.dev/colophon/). The [decisions](/design/decisions/) record every place Colophon deliberately departs from its original specification, and why.
