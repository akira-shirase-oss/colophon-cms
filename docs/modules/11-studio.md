# 11 · studio (`apps/studio`)

> The editor app. A non-technical editor can write, review, publish and understand what happened, without a terminal
> and without knowing what a cache is.

| | |
|---|---|
| Milestones | P0 (Tiptap spike), P1 (list, editor, autosave, conflicts), P2 (review, publish), P3 (receipts), P4 (history, time travel, media, preview), P5 (tokens, optional AI panel), P6 (polish, usability test) |
| Proposed owner | Claude |
| Depends on | api (`/api/v1`), sdk, ui (Material Design 3 tokens on Bits UI, D-011), content-renderer; SvelteKit + TypeScript + Vite, Tailwind, Tiptap |
| Used by | editors, reviewers, publishers, developers (developer area) |
| Spec | §04, §05, §06, §14 |

## Why it exists

Most CMS failures that editors feel are honesty failures: a button that spins forever, "published" when the site
didn't change, or a save that silently overwrote a colleague. The Studio is where Colophon's correctness becomes
visible to people who will never read the code. It is also the project's visual face for the portfolio.

## End goal

An editor drafts, previews, reviews, publishes and understands the delivery state without developer help. Five
representative editors attempt the flow, and at least four complete it without intervention (spec §05). Keyboard and
screen-reader journeys pass, and no screen ever shows a success the server has not acknowledged.

## Responsibilities

- First-run setup.
- The content list, the editor and the review screen.
- Publishing status and delivery receipts.
- History and time travel.
- The media picker and preview.
- The developer area.
- Editing safety and accessibility.
- Applying the Material Design 3 system from [ui](14-ui.md) consistently: navigation rail or drawer, top app bar,
  surface containers for panels, snackbars for acknowledgements, and dialogs only for decisions that need one.

## Not its job

- Authority. Every rule is enforced by the Rust API; client validation only improves feedback.
- Server-side rendering or database access. It is a static single-page app.
- Live co-editing or cursors (spec §01 non-goal).

## Breakdown

### Shape (spec §02)
- A SvelteKit single-page app built as static assets and served by the Rust server on the administration origin, with
  `/api` routed to Rust on the same origin.
- The single-page-app trade-offs (startup time) are accepted only for this authenticated app. Public pages are
  prerendered elsewhere.

### First run (spec §05)
- Choose an enabled template, the site name, a language and brand settings.
- The editor's starting screen offers *Create article*, *Upload media* and *Ask AI* (the last only if automation is
  enabled). It never shows database or cache settings.

### Screens

| Screen | Required behaviour |
|---|---|
| Content list | Search, draft/published filters, clear ownership, revision status, bounded bulk actions |
| Editor | Title and schema fields, Tiptap rich text with approved blocks, media picker, accessible errors, autosave state, preview |
| Review | Exactly which revisions and assets will be published, validation failures, required approvals |
| Publishing | One of five labels (below), plus the delivery-receipt timeline |
| History | Who changed what, human or agent attribution, revision comparison, restore as a new revision |
| Time travel | A release slider: view the site or an entry as it was at generation N; roll back as a new release |
| Developer area | Schemas (read-only or safe additions), destinations, machine credentials |
| AI panel (optional, P5) | Suggest, improve or draft; shows sources, unsupported claims and the diff; accept or reject |

### Publishing labels (spec §05)
- **Saved**
- **Published in CMS**
- **Website update pending**
- **Verified at configured locations** (with the actual location count)
- **Needs attention** (with a safe next action)

Internal job terminology stays behind a technical detail drawer for developers.

### Editing safety (spec §05, §06)
- Autosave is debounced and sends the expected revision.
- A concurrent save returns 409 and opens a **conflict screen**. It keeps the local proposed edit, shows the current
  server revision, and offers an explicit merge or discard. Nothing is overwritten silently.
- A local recovery copy is kept only with a clear expiry and consent policy, and never implies server persistence.
- Leaving a page with unsaved changes asks for confirmation or an explicit discard.
- **Saved** appears only after the server acknowledges.
- No indefinitely spinning buttons. Every wait has a timeout and a plain explanation with a next action.

### Accessibility (spec §05)
- Keyboard operation, meaningful focus states, labelled controls and screen-reader announcements.
- Readable contrast, reduced motion and mobile review.
- Tested with real editors and assistive technology, in addition to automated checks.

## Invariants

| ID | Invariant |
|---|---|
| ST1 | The UI never shows "Saved" or "Published" before the server acknowledges it. |
| ST2 | A conflict never discards the editor's local change without an explicit choice. |
| ST3 | "Verified" always states how many locations were checked. |
| ST4 | Every pending state has a timeout and a next action; no infinite spinners. |
| ST5 | The Studio never holds a broad administrator token in its bundle. |

## Interfaces

- `/api/v1` through the generated [sdk](13-sdk.md), with a session cookie and CSRF token.
- The preview origin, through capabilities ([auth](03-auth.md)).
- Shared [ui](14-ui.md) components and the [content-renderer](12-content-renderer.md) for previews.

## Failure modes

| Situation | Behaviour |
|---|---|
| Concurrent edit | Conflict screen with both versions; explicit merge or discard |
| Network loss during autosave | "Not saved yet"; retry with the same idempotency key; local recovery copy if consented |
| Publish accepted but delivery failing | "Website update pending" or "Needs attention" with retry or reconcile actions |
| Session expired | Prompt to sign in again while keeping the unsaved edit locally |
| API unavailable | Plain message and a next action; drafts are not claimed as saved |

## Tests and evidence

- Playwright journeys:
  - draft → preview → review → publish;
  - the conflict flow;
  - upload;
  - a preview link;
  - a delivery failure becoming visible;
  - keyboard-only completion.
- Vitest component tests.
- Automated accessibility checks on every screen.
- Budgets for bundle size, startup on a mobile network profile, and input latency. These are recorded separately from
  backend numbers (spec §28).
- The usability gate: five representative editors, at least four complete without help, failures written up.

## Build order

1. **P0:** the Tiptap spike (custom blocks, paste, image placeholder, undo/redo, serialization). Built in step 0.4:
   `packages/editor` and `apps/studio`; documents open only if they survive the editor byte for byte (D-022).
2. **P1:** content list, editor, autosave, conflict screen.
3. **P2:** review and publish.
4. **P3:** publishing labels and the receipt timeline.
5. **P4:** history, time-travel slider, media picker, preview.
6. **P5:** developer area for tokens; the optional AI panel.
7. **P6:** polish and the usability test.

## Open decisions

- **O-007:** the frontend package manager and the server-state library.
- The v0.1 rich-text block set.
- How much of time travel is a full site view vs a per-entry view in v0.1.
