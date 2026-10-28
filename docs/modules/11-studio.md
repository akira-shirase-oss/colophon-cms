# 11 · studio (`apps/studio`)

> The editor app. A non-technical editor can write, review, publish and understand what happened, without a terminal
> and without knowing what a cache is.

| | |
|---|---|
| Milestones | P0 (Tiptap spike), P1 (list, editor, autosave, conflicts), P2 (review, publish), P3 (receipts), P4 (history, time travel, media, preview), P5 (tokens, optional AI panel), P6 (polish, usability test) |
| Proposed owner | Claude |
| Depends on | api (`/api/v1`), sdk, ui (Colophon's design system on Tailwind CSS and shadcn-svelte, D-053), content-renderer; SvelteKit + TypeScript + Vite, TanStack Query (D-030), Tiptap |
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
- Applying the design system from [ui](14-ui.md) consistently (D-053): the inset sidebar shell, page headers, tables,
  form rows with the label on the left, state pills that always carry a word, toasts for acknowledgements, and dialogs
  only for decisions or short forms.

## Not its job

- Authority. Every rule is enforced by the Rust API; client validation only improves feedback.
- Server-side rendering or database access. It is a static single-page app.
- Live co-editing or cursors (spec §01 non-goal).

## Breakdown

### Shape (spec §02)
- A SvelteKit single-page app built as static assets and served by the Rust server on the administration origin, with
  `/api` routed to Rust on the same origin.
- As built (step 1.3, D-037): `colophon-server` serves a build from `COLOPHON_STUDIO_DIR` at `/studio`, with the
  app's `200.html` for client-side routes, year-long caching for hashed assets and revalidation for pages. Screens so
  far: Projects (`/studio/`), a project's content types (`/studio/projects/{project}`), a collection
  (`…/{collection}`), the editor (`…/{collection}/{entry}`), Members (`…/members`), joining a project
  (`/studio/invite`), approving a terminal (`/studio/device`), and the editor playground (`/studio/playground`).
  Content-type slugs `members` and `settings` are reserved, so these pages can sit beside collections (D-042).
  Since step 1.8 (D-043), the home page lets an administrator create a project, with the slug following the name
  until edited, and tells everyone else to ask a project's owner for an invitation.
  Since step 2.1 (D-044): `/studio/projects/{project}/changesets` lists changesets with their state and makes new ones
  from picked entries; `…/changesets/{id}` shows the pinned entries (marking any edited since), submits, refreshes
  and, for owners and publishers, approves or asks for changes to the version on screen. If the changeset changed
  meanwhile, the page shows the current version and says so instead of acting on it. Its test runs that in two tabs.
  Since step 2.2 (D-045): an approved changeset has a Publish button, which publishes on the latest release the page
  has seen and, if another release came first, says so and lets you look again; `…/releases` lists releases and
  links the public `/content/v1` address. Since step 2.3 (D-046), each release shows how its announcement to each
  destination went (refreshing while any is still under way), and owners and developers add destinations and turn
  them on or off there.
  Since step 3.5 (D-053): every project page sits in one shell: a sidebar with a project switcher, each collection
  with its entry count, changesets awaiting review, releases, members and settings, and at its foot the live release
  and its label; breadcrumbs; and a command palette (⌘K) that finds entries by title across collections. The
  Overview shows the live release, what's waiting for review, entries by state and recent edits. A collection is a
  table with title search (the API's `q`, so it finds beyond the first page) and Draft / Changed / Live filters; an
  entry's state compares its draft with its published revision. The editor puts each field in a row, gives rich text
  the playground's formatting toolbar, shows save status and live state beside the fields, and compares any revision
  with the draft. A changeset shows, for every item, what changes for readers: a word-level comparison from the live
  revision to the pinned one (new entries are all additions; unpublishing shows what goes). Destinations moved to
  `…/settings`.
- Finishing the Studio (branch `studio/dashboard`, 2026-10-27), slice by slice:
  - **Sign out from any screen** (`shell/AccountMenu.svelte`, in the project shell's header and the plain frame's):
    who is signed in, *Your work and projects*, and *Sign out*, which calls `POST /auth/logout` and only then
    reloads at the front door with a note that says so. If the server can't be reached it says you're still signed
    in. The identity provider's own session is not ended (no RP-initiated logout yet), so the note tells people on
    a shared computer to close the browser. Test: `tests/server/signout.spec.ts`.
  - **Honest states and permission-aware navigation**: the sidebar, the Overview, the command palette and the new
    publish request dialog show only collections the role may read (the server refuses the others' lists), so the
    Overview and its checklist load for a role that can't read every collection. Each Overview section waits only for
    what it shows and shows a failed read instead of "Loading…"; the palette searches each collection on its own and
    says which it couldn't. `shell/Problem.svelte` words every failure through `lib/errors.ts` (a 403 is "Your role
    can't see this"), offers *Try again* when that can help, and keeps the server's words in Technical details.
    Test: `tests/server/honest.spec.ts`.
  - **Plain-words errors**: `lib/errors.ts` turns a failure into a title, what happened and what to do, by status and
    `details.reason`; the few 409s that have no reason yet are matched by the server's phrase until B3 gives every
    refusal a stable reason. Used by the publish request page, one-click publishing, scheduling, Members and new
    entries. Tests in `tests/server/words.spec.ts`: a review refused because an entry was edited meanwhile (409) and a
    request refused because the role was narrowed meanwhile (403) both read without internal terms.
  - **Controls follow permissions**: *Ask for review* and *Update to the latest versions* need edit rights in every
    collection the request touches (a role that can't edit is told who can); *Cancel the schedule* shows for its author
    or someone who may publish it; Settings (sidebar and search) shows only to a role that can change something there
    (`SETTINGS_ACTIONS`: content types, website connections, roles, tokens), so Editors don't see it. Built-in Editors
    may read, create and update everywhere, so they still send and update requests. Test: `tests/server/controls.spec.ts`.
- The single-page-app trade-offs (startup time) are accepted only for this authenticated app. Public pages are
  prerendered elsewhere.

### First run (spec §05)
- Choose an enabled template, the site name, a language and brand settings.
- The editor's starting screen offers *Create article*, *Upload media* and *Ask AI* (the last only if automation is
  enabled). It never shows database or cache settings.
- As built in step 3.7 (D-059, D-061), from sign-up to a first release:
  - **The front door** (`/studio/`): signed out, *Sign in* and *Create an account* (the identity provider's own
    sign-up form, `/auth/login?start=create`). On an installation with no administrator, a signed-in person sees
    *Finish setting up Colophon*: a field for the setup code the server printed to its terminal, and, in Technical
    details, `colophon admin bootstrap --subject <their subject>`. An administrator with no project gets *Create your
    first project*; anyone else, *You're not in a project yet*, with the name to give and the role to ask for. The
    invitation page offers an account too, so an invited person can sign up from the link.
  - **Starting points** (`lib/starters.ts`, `NewProject.svelte`): a small blog (Articles get a required URL name
    from the title), a documentation site (a Pages type: title, URL name, body) or blank. Applied right after the
    project is made, as its owner, through the content-type API; if that part fails, the project exists and a toast
    says how to finish it in Settings.
  - **The first-run checklist** on the overview (`FirstRun.svelte`): write the first article (or page), publish it
    (or ask for review, when the role can't approve and publish), connect the website, invite a teammate, see the
    first receipt. Each item is derived from real state (entries, publish requests, releases, website connections,
    members and invitations) except two things the server can't know: whether this person hid the list and whether
    they have opened a receipt, both kept in this browser (`lib/firstRun.ts`). Items a role can't do are left out.
  - **One-click publishing** in the editor (`PublishEntry.svelte`): *Publish* makes a publish request for the saved
    version, sends it for review, approves it as a recorded self-review and publishes it on the release the page has
    seen, when the project allows approving your own requests (`self_review` on the project, D-044) and the role may
    approve and publish in that collection; otherwise *Ask for review* stops after sending it. Every step is the
    ordinary API call; if another release went live first (D-045) it stops, says so, and links to the request.
  - **Plain words** (`lib/words.ts`, D-059 rule 1): publish request, version, URL name, website connections (site
    rebuild, CDN clear, site check), releases by number. Ids, hashes and API addresses moved into *Technical
    details* drawers (`shell/TechnicalDetails.svelte`). Routes and test ids keep their internal names.
  - **Teaching empty states** (`shell/EmptyState.svelte` with an action): articles, publish requests, releases,
    members (only you), website connections, pending invitations, the overview's recent edits.

### Screens

| Screen | Required behaviour |
|---|---|
| Content list | Search, draft/published filters, clear ownership, revision status, bounded bulk actions |
| Editor | Title and schema fields, Tiptap rich text with approved blocks, media picker, accessible errors, autosave state, preview. Since step 1.5 every field is drawn from the entry's content type (D-040) |
| Review | Exactly which revisions and assets will be published, validation failures, required approvals |
| Publishing | One of five labels (below), plus the delivery-receipt timeline. Since D-060, an approved changeset's page has a Schedule panel (`SchedulePanel.svelte`): a date, a time and a time zone (the person's own first); a time the clocks show twice or skip shows both moments, with their zone abbreviations, and schedules the one picked; a pending schedule shows when it publishes (there and here), who set it, and can be cancelled; past ones say published on time, published late, not published and why, or cancelled. Releases lists scheduled ones, then recent outcomes, and reads the new release when one publishes |
| History | Who changed what, human or agent attribution, revision comparison, restore as a new revision |
| Time travel | A release slider: view the site or an entry as it was at generation N; roll back as a new release |
| Developer area | Schemas, destinations, machine credentials. Since step 3.6 (D-054, D-055) the content-type builder at Settings → Content types (`/studio/projects/{project}/settings/types/{type}`, `TypeBuilder.svelte`): rename the type and its fields, add fields of every type, change limits and requiredness, remove fields; the server decides what is safe, and a refusal lists the entries in the way with links. Field ids and types and the title field are fixed once saved. Slug fields get a "From title" suggestion in the editor (`slugify`, matching the server's rule) |
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
- As built (step 1.3, D-038): `src/lib/saver.svelte.ts` keeps edits in this browser first, saves a second after typing
  stops (one save at a time; ⌘S or **Save now** at once), reuses the `Idempotency-Key` while the base and content are
  unchanged, and times every request out after 15 s. A 409 opens the conflict screen (both versions, rendered by the
  content renderer): **Keep my version** or **Use their version**. Kept edits come back after a reload and expire
  after 30 days. Two deliberate differences from the bullets above: no consent prompt for the local copy, and leaving
  only warns when the copy couldn't be kept (D-038).

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
- In place since step 1.3 (`tests/server/`, `scripts/studio.sh`, against the release server and the stack): autosave
  and reload; F04 in two browsers, keeping mine and using theirs; offline edits saved when the network returns; edits
  restored after an immediate reload; a save whose answer is dropped is retried with the same key and replayed; kept
  copies expire; axe finds nothing on the home page (signed out and in), the editor or the conflict screen.
- Since step 1.4: a save that committed just before a reload, with someone else's save on top, is resent with its key
  on reopening and replayed, with no conflict and no second copy (the case the simulator found, D-039).
- Since step 1.6: `/studio/device` approves a terminal's sign-in (D-041); its test allows a device and uses the token it
  produces.
- Since step 3.7 (D-059, D-061): `tests/server/journey.spec.ts` runs the whole first-run journey on a new
  installation (its own database, migrated and never bootstrapped, and its own server on port 58402, whose setup
  code it reads from the server's standard error): a new person signs up, enters a wrong code then the right one,
  creates a project from the blog starting point, writes an article (its URL name made from the title), publishes it
  in one click, opens its receipt, invites a teammate; the teammate signs up, sees how to join, joins from the link
  as an editor and asks for review; the owner approves and publishes it; the hidden checklist stays hidden. axe finds
  nothing on any of its screens in light or dark, and each is photographed when `STUDIO_SHOTS_DIR` is set.
  `tests/server/words.spec.ts` fails on an internal term in visible text, `aria-label`, `title` or placeholder on
  every everyday screen and empty state, checks that the Technical details drawer has them, and checks the check
  sees them in Settings. `scripts/first-run.sh` starts such an installation by hand on port 58400.
- Since step 1.7 (D-042): `/studio/projects/{project}/members` lists members, and lets owners change roles, remove
  people, and create and revoke invitation links (each shown once). `/studio/invite` reads the token from the link's
  fragment, keeps it across sign-in, shows the project, role and inviter, and joins only on Join. Its test runs the
  whole story in two browsers: invite, sign in from the link, join, an editor with nothing to manage, the last owner
  refused, a revoked link, leaving; axe finds nothing on either page.
- Automated accessibility checks on every screen.
- Budgets for bundle size, startup on a mobile network profile, and input latency. These are recorded separately from
  backend numbers (spec §28).
- The usability gate: five representative editors, at least four complete without help, failures written up.

## Build order

1. **P0:** the Tiptap spike (custom blocks, paste, image placeholder, undo/redo, serialization). Built in step 0.4:
   `packages/editor` and `apps/studio`; documents open only if they survive the editor byte for byte (D-022).
2. **P1:** content list, editor, autosave, conflict screen (step 1.3, done: D-037, D-038); entries of any content
   type (step 1.5, done). Sign-out (done, see above) and a content-type designer (today definitions are written as JSON through the API
   and the testbed) come next; the formatting toolbar and search arrived in step 3.5.
3. **P2:** review and publish.
4. **P3:** publishing labels and the receipt timeline. Step 3.3 (D-051): each release on the Releases page shows its
   label (*Published in CMS*, *Website update pending*, *Verified at N locations*, *Needs attention*), linking to its
   receipt: each destination's part and every probe look, oldest first. Probes are added with the other
   destinations, optionally pinned to a location. Step 3.5 (D-053): the redesign above, with the formatting toolbar,
   search, entry states, revision comparison and review diffs. Step 3.6: the content-type designer.
5. **P4:** history, time-travel slider, media picker, preview.
6. **P5:** developer area for tokens; the optional AI panel.
7. **P6:** polish and the usability test.

## Open decisions

- The v0.1 rich-text block set.
- How much of time travel is a full site view vs a per-entry view in v0.1.
