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
    in. Since f4338a7 the answer names the identity provider's sign-out address, and the browser follows it
    (Keycloak asks to confirm, then returns to `/studio/`), so signing in again asks for a password; only when the
    provider gives no address does the front door say to close the browser on a shared computer. Test:
    `tests/server/signout.spec.ts`.
  - **Honest states and permission-aware navigation**: the sidebar, the Overview, the command palette and the new
    publish request dialog show only collections the role may read (the server refuses the others' lists), so the
    Overview and its checklist load for a role that can't read every collection. Each Overview section waits only for
    what it shows and shows a failed read instead of "Loading…"; the palette searches each collection on its own and
    says which it couldn't. `shell/Problem.svelte` words every failure through `lib/errors.ts` (a 403 is "Your role
    can't see this"), offers *Try again* when that can help, and keeps the server's words in Technical details.
  - **Gaps against the API** (the audit below), closed one by one: a published entry's page says *Where readers get
    it* (its public `/content/v1` address, to open or copy; with newer changes, that readers get the version live in
    its release). Test: `tests/server/address.spec.ts`.
    Test: `tests/server/honest.spec.ts`.
  - **Plain-words errors**: `lib/errors.ts` turns a failure into a title, what happened and what to do, by status and
    `details.reason`, which the server gives every 403 and 409 (a missing permission also names it), never by the
    server's wording. Used by the publish request page, one-click publishing, scheduling, Members and new
    entries. Tests in `tests/server/words.spec.ts`: a review refused because an entry was edited meanwhile (409) and a
    request refused because the role was narrowed meanwhile (403) both read without internal terms.
  - **Controls follow permissions**: *Ask for review* and *Update to the latest versions* need edit rights in every
    collection the request touches (a role that can't edit is told who can); *Cancel the schedule* shows for its author
    or someone who may publish it; Settings (sidebar and search) shows only to a role that can change something there
    (`SETTINGS_ACTIONS`: content types, website connections, roles, tokens), so Editors don't see it. Built-in Editors
    may read, create and update everywhere, so they still send and update requests. Test: `tests/server/controls.spec.ts`.
  - The editor reads an entry's live state from the entry itself (`published` on a read), not from the collection's
    newest 100, so an older entry no longer shows as Draft.
  - **Your work** (`/studio/`, `lib/MyWork.svelte`): signed in with a project, the front door shows the person's work
    in the project they last opened in this browser (`lib/lastProject.ts`; a picker when they have several, none when
    they have one): entries they started that aren't live, publish requests waiting for review (worded for whether they can
    approve them) and their own still in progress (`created_by`/`submitted_by`), upcoming and blocked schedules with
    why, and the last three releases with their labels; then, with several projects, every project. A list row whose
    title the role can't read (or that has none) says "No title to show". Test: `tests/server/mywork.spec.ts`.
  - **The editor, finished for P3**: *Unpublish* in one click beside *Publish* (a request whose one item takes the live
    version down, sent, approved and published like publishing; *Ask to unpublish* when the role can't approve), undone
    by publishing again; *Restore this version* in History (its fields become the draft and are saved as a new
    version on top, so nothing is lost; locked fields keep their values); and *Load more* on a collection, following
    the server's cursor 100 at a time. Test: `tests/server/entry-actions.spec.ts`.
  - **Bulk actions on a collection**: pick rows (or every row shown) and *Publish N* together, as one publish request
    and one release, or *Ask for review of N* when the role can't approve; at most 100 at a time, and live entries with
    nothing new are left out and said so. *Duplicate* on each row copies what the role can read, adds "(copy)" to the
    title and takes the next free URL name, and opens the copy. One-click publishing lives in `lib/publishing.ts`,
    shared with the editor. Test: `tests/server/bulk.spec.ts`.
  - **Phone-friendly review** (closes ROADMAP 3.7's open item): below `xl` the editor's save status and Publish /
    Unpublish are a bar that stays at the bottom of the screen while the fields scroll (the details and Technical
    details stay on the wide card); below `lg` a publish request's Review panel comes first, so Approve is on the first
    screen. `tests/server/journey.spec.ts` now opens every project screen at 390x844 (home, overview, a collection,
    the editor, publish requests, a request waiting for review, releases, a receipt, members, settings): no sideways
    scroll, axe in light and dark, plain words outside Settings, Publish and Approve in view without scrolling.
  - **Receipt next steps**: a receipt that *Needs attention* says which website connection gave up and why in plain
    words (`failureText` in `lib/receipts.ts`: "your website refused it (HTTP 404)", "couldn't reach your website"…),
    that the release is published in Colophon but the website may not show it, and what to do (check the connection,
    publish again: the next release carries this one's changes), with *Open website connections* for roles that
    manage them. Releases says failures the same way and shows schedule and delivery load errors. A published
    request's "release N" opens that release's receipt. A retry button per delivery waits for the API's retry (B5).
    Test: `tests/server/deliveries.spec.ts` (a webhook answering 404).
- Step R.4 in the Studio (API tokens and the audit log; the API is in [api](04-api.md)):
  - **The role editor** names the new `audit.read` as *See the activity log*, and *Manage API tokens* says what it
    means. Its grid is now `PermissionGrid.svelte`, with the words in `lib/permissions.ts`, shared with custom API
    tokens and the activity log's sentences. Test: `tests/server/permissions.spec.ts`.
  - **Settings → API tokens** (`…/settings/tokens`, for roles that manage tokens; others are told why it's empty):
    each token with its state (*Active*, *Ended*, *Revoked*), what it may do, last use and end date in words; making
    one as *Read only*, *Full access* or *Custom* (the role editor's grid without the four powers only people hold)
    for 7 days to a year; its secret shown once with *Copy* and "You won't see it again"; *Revoke* asks first.
    Refusals use B3's reasons (`cannot_grant`, `not_for_tokens`, `too_many_tokens`…). Test: `tests/server/tokens.spec.ts`
    (the token reads with `Authorization: Bearer`, is refused a write, shows its last use, and answers 401 once
    revoked; a custom token creates entries and is refused people's routes; phone width).
  - **Settings → Activity** (`…/settings/activity`, the audit log, for roles with `audit.read`): each change as a
    sentence (`lib/activity.ts`: "Dev Owner changed Dev Editor's role from Editor to Desk"), newest first and grouped by
    day; filters for the kind of change and the person; *Show older* pages back 25 at a time. People are named from
    the members list and the events' own actors, so someone who left is usually still named. Settings now shows to
    roles with `audit.read` too. Test: `tests/server/activity.spec.ts` (seven changes in order, both filters, paging,
    phone width; an Editor sees neither page, in the sidebar, in search or by address, and the Studio doesn't ask).
  - **Own-only roles and "Your drafts", with the API's creators** (790c450): *Ask for review* and *Update* on a
    publish request, the editor's *Ask for review*, and bulk sending follow permissions limited to one's own entries
    (`mayOn`, with each item's and entry's `created_by`). "Your drafts" lists the entries the person started
    (`?created_by=`), however many newer ones others wrote. Tests: `tests/server/controls.spec.ts`,
    `tests/server/mywork.spec.ts`.
- Step M.1 in the Studio (D-064; the field types are in [domain](01-domain.md)):
  - **The content-type builder** offers the seven new types in plain words (*Email address*, *Web address*, *Date*,
    *Time of day*, *Structured data (JSON)*, *Markdown text*, *Colour*), markdown's character limit, an optional
    minimum on text, and *Help for editors* on any field; help text is sent trimmed and only when there is some, and
    the minimum only when set, as the server writes definitions. Test: `tests/server/fields.spec.ts`.
  - **The entry editor** draws each with a fitting control: native inputs for email, web address, date, time and
    colour (a swatch beside a `#rrggbb` box), a textarea with a live character count for markdown, and a monospaced
    one for JSON; each field's help text sits under its label. Checks mirror the server's (`fieldProblem` in
    `lib/content.ts`) and show under the field; the server still decides, and its refusal is marked on the field it
    names, in plain words. Values are sent in the server's canonical forms (a web address as `new URL(text).href`,
    shown so once it is one; colours lowercase; JSON by `lib/json.ts`, which writes `write_json`'s bytes), and
    `canonical()` compares JSON canonically, so a value read back is never a change. JSON that doesn't read yet is
    never sent: the save is *held* (`Saver.hold`), says why, and leaving the page warns. Tests:
    `tests/json.spec.ts` (the canonical form, the number rules, depth and size), `tests/server/fields.spec.ts`
    (values stored canonically and nothing to save after a reload, plain refusals, and the server's stored bytes
    equal to `canonicalJson` for a value with astral and non-ASCII keys, `1.5` and `-0`).
- Step M.2 in the Studio (D-065; links are in [domain](01-domain.md)):
  - **Link fields**: the builder's *Link to entries* type chooses the collection it links to (fixed once saved) and
    *One entry* or *A list of up to N*. In the editor, `ReferencePicker.svelte` searches the target collection's
    titles (the API's `q`) and sets the id; a list shows chips that move up and down and are removed, up to its
    maximum and never twice. Chosen entries are named by reading each; one the role can't read is "An entry you
    can't see", never its id. An id the server refuses (`reference_not_found` at `/related/1`) is marked on its
    chip and in plain words under the field. Test: `tests/server/references.spec.ts`.
  - **Link refusals by name** (`lib/references.ts`): the server's `references_unpublished` and `still_referenced`
    name no entries, so the Studio works them out from the request's own: the linked entries its pinned versions
    name that aren't published and aren't in it, and the published entries (`referenced_by`) that still link to what
    it takes down. One-click publishing and the publish request page say, for example, "“Notes on the engine” links to
    “Ada Lovelace”, which isn't published yet. Publish “Ada Lovelace” first, or in the same publish request.", and
    the request page offers *Make a publish request that includes* them. Test: `tests/server/references.spec.ts`.
  - **Linked from** (624f6bd): an entry's editor lists the entries that link to it (its read's `referenced_by`),
    each named by reading it, with the field and whether readers see the link. The server lists only collections the
    role reads in full, so a role that reads only its own entries isn't told which link here. When readers see a
    link, *Unpublish* explains that it will be refused and names the linking entries before it is tried (*Keep it* or
    *Try anyway*). Test: `tests/server/references.spec.ts`.
- Step M.3 in the Studio (D-066; structures are in [domain](01-domain.md)):
  - **The builder** offers *Group of fields* (object), *List* (what each item is: any field kind but a list, page
    sections or a URL name, or a group of fields; a maximum) and *Page sections* (blocks: kinds of section, each with
    its own fields; a maximum). Field lists are edited by `FieldsEditor.svelte`, recursively (rules and words in
    `lib/fieldDefs.ts`). Structures are offered only while they may still nest (three deep at most) and the builder
    says so where it stops; an empty group, a kind of section without fields and page sections without kinds are
    said before saving, and Save waits. Test: `tests/server/structures.spec.ts`.
  - **The editor** draws structures with `StructureInput.svelte`: a group of fields as a fieldset; a list as rows that
    are added, moved and removed, up to its maximum (an added item isn't saved until something is typed into it);
    page sections as a stack, each with its kind's label, collapsible, movable and removable, added by choosing a
    kind. Inside, every control is the editor's own (FieldInputs, recursively; rich text through
    `RichTextInput.svelte`; links through the picker), each at its JSON Pointer (`/faq/1/q`, `/sections/0/by`), so a
    refusal lands under it. Values are built in definition order with `_block` first; `canonical()` compares
    structures by their definitions' order; JSON held anywhere inside holds the save; comparisons and the conflict
    screen read structures as their parts. "Linked from" names nested places ("Pages, in Sections → Quote 1 → By",
    the kind from the linking entry's content; `placeName` in `lib/references.ts`). Test:
    `tests/server/structures.spec.ts` (two kinds of section, a list of questions, a list of tags and a quote linking
    to a person, filled in and reordered, stored canonically, nothing to save after a reload, a refusal at `/faq/1/q`
    under its control, phone width, axe, plain words, and the person's "Linked from").
- Step M.4 in the Studio (D-067): **single types**. The builder's switch *Just one entry (like site settings or a home
  page)* writes `single: true` (left out otherwise). The sidebar opens a single type's one entry directly, and its page
  redirects there; with none yet, it offers *Create it*. A second entry (409 `single_exists`) is refused as "This
  already has its entry; edit that one." with *Open it*; making a type with several entries single (422
  `too_many_entries`) is said plainly in the builder. Test: `tests/server/single.spec.ts`.
- D-069 in the Studio: **allowed forms of text**. Every text field in the builder, at any depth, has *Allowed form
  (optional)*, a regular expression matched against the whole value, left out of the definition when empty and kept
  when other options change; a form the server can't use (422 `invalid_pattern` at `/fields/n/pattern`, or deeper) is
  said plainly at that input. The editor never imitates the regex: a value the server refuses (`pattern_mismatch`) is
  said under the field as "Code isn't in its allowed form." with the field's help text. The words are in
  `lib/words.ts` (`ALLOWED_FORM`, `INVALID_PATTERN`, `patternMismatch`). Tests: `tests/patterns.spec.ts`,
  `tests/server/patterns.spec.ts`.
- D-070 in the Studio: **unique fields**. The builder offers *No two entries may share this* only on top-level
  single-line text, email, web address, number, date, time and date-and-time fields (`canBeUnique` in
  `lib/fieldDefs.ts`), writes `unique: true` only when on, and says at the switch when it can't hold (`invalid_unique`)
  or when entries share a value (`needs_migration`, the entries listed with "It shares its Code with another entry.").
  In the editor, `value_taken` is said under the field ("Another entry already has this Email.") with *Open “Ada”*
  only when the server names the entry, which it does only to a role that reads it; a URL name taken by an entry the
  role can't read is refused without its name. A release that would publish a value another entry still publishes is
  said in the publish flow's words. Words in `lib/words.ts`. Tests: `tests/unique.spec.ts`,
  `tests/server/unique.spec.ts`.
- D-071 in the Studio: **field defaults**. The builder's *Default value (optional)* uses the field's own kind of input
  (`defaultInput` in `lib/fieldDefs.ts`: a text box, a number box, Yes/No, the choices, date, time and date-time
  pickers, a colour), only on top-level fields that aren't unique and can have one (`canHaveDefault`); cleared, it's
  left out; `invalid_default` is said at that input. A new entry leaves defaulted fields to the server, which fills
  them on creation (the starter no longer picks a required choice's first option or false over a default), and the
  new-entry dialog shows a required field's default already chosen. Entries written before are never touched. Tests:
  `tests/defaults.spec.ts`, `tests/server/defaults.spec.ts`.
- D-072 in the Studio: **conditional fields**. The builder's *Show only when* (on top-level fields but the title) picks a
  yes-or-no or choice field that always applies and ticks its values, kept in that field's order; *Always shown*
  removes it; a field others depend on says it always shows; `invalid_condition` is said at the control. In the
  editor (`applies` in `lib/content.ts`, the server's rule), a field shows only while its condition holds, is required
  only then, and when it stops applying its value is cleared and said so quietly ("Duration is cleared because it only
  applies when Kind is video or podcast."), so a save never earns `not_applicable`; the new-entry dialog asks only for
  fields that apply and sends nothing that doesn't. A condition stored entries don't fit is listed like other changes
  in the way. Tests: `tests/conditions.spec.ts`, `tests/server/conditions.spec.ts`.
- D-073 in the Studio: **shared components**. Settings lists them (fields, version, and the content types that use
  each, worked out from their definitions by `usedBy` in `lib/fieldDefs.ts`) and makes one from a name and a name in
  code, starting with a Title field. Its page (`settings/components/[component]`, `lib/ComponentBuilder.svelte`) edits
  its fields like a group's, one structure deep, so nothing unique, no defaults, conditions, URL names or components
  inside are offered. A save says the new version of every content type that changed with it, with links; a change
  their entries can't take names that content type, linked, from the server's `details.content_type`, and lists its
  entries with links; a type's own refusal (too deep there, say) is named the same way, with the server's words. A
  content type's builder offers *Component: SEO* wherever a group can go and the component's own
  structures still fit three deep (`componentFits`), shows its fields read-only with a link to its page, and the
  entry editor fills it like any group. Changing one takes `schema.change` everywhere; a role without it reads it,
  and the server refuses its change too. Tests: `tests/components.spec.ts`, `tests/server/components.spec.ts`.
- D-076 in the Studio: **rate limits**. The one fetch layer (`send` in `lib/api.ts`) reads a 429 `rate_limited` and
  its `Retry-After` (`retryAfterMs`: seconds or a date, a second if neither, at most a minute). A read waits that long
  and goes once more, then fails with the wait (TanStack Query doesn't retry it again); a write isn't resent there, so
  its caller decides, and every attempt is the same request, Idempotency-Key included. The autosave waits as long as
  the server asked (an edit or ⌘S meanwhile doesn't go sooner), says "Colophon is busy for a moment; trying again in 1
  second. Your changes are kept on this device.", keeps the edit and the request in flight on the device, and sends the
  same save again, so a retried save can't be stored twice. Elsewhere the words are "…; try again in N seconds." (title
  "Colophon is busy"). Other 429s, such as too many setup code tries, keep their own words. Words: `busyFor` in
  `lib/words.ts`. Tests: `tests/ratelimit.spec.ts` (the fetch layer with a stand-in `fetch`) and
  `tests/server/ratelimit.spec.ts`, which starts a server of its own with `COLOPHON_RATE_LIMIT_PER_CREDENTIAL=1/40`
  (and no limit per address) on `COLOPHON_JOURNEY_PORT`, uses up the session's limit and shows an edit surviving the
  429.
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

## Gaps against the API (audit, 2026-10-28)

Every route in `crates/colophon-api/src/lib.rs` and every row of `docs/COMPARISON.md`'s parity backlog, checked
against what the Studio shows or does. Sizes: S (a day or less, Studio only), M (a few days, or needs a backend route
first), L (a milestone).

Backend capabilities with no Studio screen or action:

| Capability | What's missing | Size |
|---|---|---|
| An entry's public address (`/content/v1/{project}/{environment}/{collection}/{entry}`) | **Done** (2026-10-28): a published entry's page says *Where readers get it*, to open or copy; with newer changes it says readers get the version live in that release; a never-published draft shows none (`tests/server/address.spec.ts`) | S |
| `created_by` on an entry list | My work uses it; a collection's list can't be narrowed to one's own entries | S |
| `GET /version` (version, commit, build date, API versions) | Shown nowhere, so a problem report can't say which server it was | S |
| Delivery reads (`/content/v1/...`) | Addresses only; no view of what readers get for an entry or release (the JSON, at a generation) | M |
| `GET …/jobs?generation=` | Not used; the receipt already carries each destination's state, attempts and last error | none |
| `GET …/permissions` | Not used; the Studio reads the same grants from the project | none |
| `POST /api/v1/rich-text/validate` | Not used; the Studio validates with the same package before saving | none |

Asked about, and complete for what the API offers: scheduled publishing (a local time with the two instants when
clocks change, cancel, upcoming on Releases and My work; changing a time is cancel and schedule again, as in the API);
receipts and probes (labels, each destination's part, every probe look, locations); website connections (webhook, CDN
purge, probe with a location; on and off); API tokens (read-only, full access, custom grants, expiry, last used,
revoke); the activity log (by kind and person, older pages; the API has no date range); single types; the reference
picker in groups, list items and page sections; history (compare any revision, restore as a new one); bulk publish
and unpublish from a list, and duplicate.

Needs a backend route first (each M unless said): deleting entries, content types and components; renaming or
removing a project; changing or removing a website connection (only on and off today); rotating a token; seeing and
ending one's sessions; defaults and conditions inside structures; geopoint fields. Later milestones (L): media
library (P4), localization, comments and tasks, presence, visual preview (E), query filters and webhooks by event (D).

## Open decisions

- The v0.1 rich-text block set.
- How much of time travel is a full site view vs a per-entry view in v0.1.
