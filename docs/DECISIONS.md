# Decisions and deviations from the master spec

The master spec ([spec/](spec/README.md)) remains the reference design. This file records every place where Colophon
deliberately differs from it, plus decisions that are still open. Add an entry whenever a design changes: date,
decision, reason, impact.

## Made

| ID | Date | Decision | Reason | Impact |
|---|---|---|---|---|
| D-001 | 2026-10-25 | The project is named **Colophon CMS**. | A book's colophon records how, when and by whom it was printed: proof of what was published. | Crates are `colophon-*`; the CLI binary is `colophon`; the reference site has a `/colophon` page. |
| D-002 | 2026-10-25 | The goal is an open-source and portfolio project that is genuinely useful. Selling is out of scope. | The maintainer's stated goal ([WHY.md](WHY.md)). | Scope is judged by proof and usefulness, not market fit or feature count. |
| D-003 | 2026-10-25 | **Proof over breadth**: four signature features (crash-proof publishing, deterministic simulation, time travel, delivery receipts) define the product. | Hard-to-fake evidence is what makes the project stand out ([WHY.md](WHY.md)). | Time travel and delivery receipts, already supported by the spec's data model (§07, §14), become first-class, user-visible features. |
| D-004 | 2026-10-25 | The spec's `ai` crate becomes **`colophon-automation`**. The AI writing assistant is optional, off by default, and built last. | "Not another AI jargon" project. Scoped automation for scripts and agents is the useful part. | [modules/09-automation.md](modules/09-automation.md). |
| D-005 | 2026-10-25 | New module **`colophon-sim`**: deterministic simulation harness, real-process crash loop, consistency checker. | The spec requires fault tests (§30); Colophon makes them a signature feature. | Domain logic must stay I/O-free and driveable by a virtual clock ([modules/10-sim.md](modules/10-sim.md)). |
| D-006 | 2026-10-25 | **Parked:** commerce, payments, inventory, shipping, tax, refunds, subscriptions, entitlements (§20–23); hosting adapters and custom domains (§25); membership and knowledge-base templates (§19). **Later:** forms and leads (§24), source import adapters (§12), full localization UI, schema designer. | Each is a large product surface that adds breadth, not proof. | Spec milestone M6 is out of v0.1. Parked areas keep their spec contracts for a future return ([modules/parked.md](modules/parked.md)). |
| D-007 | 2026-10-25 | Requirement owners in spec §36 are mapped onto real crates: `cache` → delivery, `policy` → auth, `templates` → web/blog template, `forms` → parked. | §36 names modules that the §03 repository layout does not define. | Module docs are the single owner map. |
| D-008 | 2026-10-25 | The CLI command lists in spec §09 and §11 are merged into one contract. `module enable` is dropped (no optional modules in v0.1). Time-travel commands are added. | The spec lists two slightly different command sets. | [modules/08-cli.md](modules/08-cli.md) is the only command reference. |
| D-009 | 2026-10-25 | Duplicate template material in spec §18 and §19 is consolidated into one blog template doc. | Avoid drift between two definitions. | [modules/15-web-blog.md](modules/15-web-blog.md). |
| D-010 | 2026-10-25 | Planning assumes ~45 hours/week. | The maintainer offered 8–16 hours/day; some gates depend on people and calendar time, not hours. | Estimates in [ROADMAP.md](ROADMAP.md). |
| D-011 | 2026-10-25 | The Studio uses **Material Design 3** as its design language: colour roles generated with `@material/material-color-utilities` from a brand seed, the M3 type scale, shape, elevation, state layers and motion, exposed as CSS custom properties and Tailwind theme tokens in `packages/ui`. Components are built on **Bits UI** headless primitives (keyboard and screen-reader behaviour), styled to the M3 specs. | The maintainer asked for M3. It is a complete, documented, accessible design system, and a token-first implementation keeps the components owned and Svelte-native. Not adopted: `@material/web`, Google's M3 web components, whose README says it is in maintenance mode pending new maintainers (checked 2026-10-25); MUI, which is React-only. | Replaces spec §02's "shadcn-svelte" styling; Tailwind and Bits UI stay. See [modules/14-ui.md](modules/14-ui.md) and [modules/11-studio.md](modules/11-studio.md). The public blog keeps its own look and may share tokens. |
| D-012 | 2026-10-25 | The logo is the octagonal C mark with the "Colophon CMS" wordmark. | Shared by the maintainer. | [brand/](brand/README.md); the SVG there is a recreation until the original files are added. |

## Open (decide at the milestone shown)

| ID | Decide by | Question | Options and notes |
|---|---|---|---|
| O-001 | P0 | Rust toolchain pin and `rust-version` | rustup has 1.98.1 and 1.97.1 installed; latest stable is 1.99.1 (rust-mastery, checked 2026-10-22). Homebrew 1.96.0 ignores `rust-toolchain.toml`, so fix `PATH` first. |
| O-002 | P0 | Redis or Valkey for the cache | Redis 8 is licensed AGPLv3/RSALv2/SSPLv1 (spec §35). Valkey is a Redis-compatible alternative. Either way, code against a Redis-compatible adapter and test compatibility. |
| O-003 | P0 | SQLx compile-time checked queries in CI | Needs a live database or committed offline query data (`cargo sqlx prepare`). |
| O-004 | P0 | OpenAPI generation crate | Not in the rust-mastery crate guide; evaluate candidates with its chapter 15 decision checklist. |
| O-005 | P0 | Domain ports: synchronous or async; generic or `dyn` | rust-mastery default: generic core, `dyn` chosen at the composition root. Simulation favours a synchronous, I/O-free core with async only at adapter edges. |
| O-006 | P0 | Content hash algorithm for revisions, changesets and ETags | SHA-256 for interoperability or BLAKE3 for speed (both in the crate guide). |
| O-007 | P0 | Frontend package manager and server-state library for the Studio | The spec requires a committed lockfile; pick one package manager for the whole repo. |
| O-008 | P1 | OIDC client crate and the local development identity | Must refuse to run the development identity on a publicly bound install (spec §13). |
| O-009 | P1 | Ownership split | Proposed: the maintainer writes `colophon-domain` and `colophon-db`; Claude builds the rest; high-risk areas get manual review. Not yet confirmed. |
| O-010 | P2 | Worker wake-up mechanism | Polling only, Postgres `LISTEN/NOTIFY`, or Redis notifications. Workers must always be able to find all work from Postgres alone. |
| O-011 | P1 | Build the deterministic scheduler or adopt a crate | Evaluate existing deterministic-simulation crates against a small hand-written scheduler. |
| O-012 | P3 | Hosting for the reference site and probe locations | One probe location means the UI must say "verified at 1 location", never "verified". |
| O-013 | P5 | MCP implementation | Evaluate the official Rust SDK against a thin JSON-RPC layer over the same services. |
| O-015 | P0 | Adopt `m3-svelte` for complex components, or build everything on Bits UI | `m3-svelte` 7.2.1 (2026-09-20) is an active Svelte M3 library, licensed Apache-2.0 OR GPL-3.0-only, with a single maintainer. Decide after the Tiptap spike, weighing accessibility, bundle size and bus factor. |
| O-016 | P0 | Brand seed colour for the M3 scheme | The logo is monochrome, so the accent has to be chosen. Check that the generated light and dark schemes meet contrast at the standard and high contrast levels. |
| O-014 | Before v0.1 | License confirmation | Apache-2.0 is proposed for the core, CLI and SDK (spec §35); the maintainer must approve it before the first public release. |
