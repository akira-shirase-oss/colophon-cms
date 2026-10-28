<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon's goal includes people who aren't developers running it without help: writing, getting a review and publishing, without learning what a changeset or a destination is. Decision D-059 (27 October 2026, the maintainer's requirement) turns that into rules every Studio screen follows, and makes the spec's usability gate (§05) binding on every step. Step 3.7 applied them, from signing up to a first release, with one check left: rule 7, every screen readable and reviewable on a phone, is checked on two screens so far.

## Why it was needed

The spec already says the Studio must not expose internal job terminology where "Website update pending" is enough, and must keep technical detail in a drawer for developers. When D-059 was written, the Studio still showed "changeset" 15 times, and "destination", "probe", "webhook" and "pinned" on everyday pages, and a fresh installation could only be set up by someone holding the database owner's credentials. The engine underneath was not the problem; the words and the number of steps were.

## The seven rules

D-059's rules for every Studio screen:

1. **Plain words, from one glossary.** A changeset is a *publish request*, a revision a *version*, a release keeps its name with the time it went live, a slug is a *URL name*, and destinations are *website connections*: rebuild the site, clear the CDN, check the site. Internal terms stay in the API, the command line and a *Technical details* drawer.
2. **One primary action per screen** (the black pill button), and a guided path: write, review, publish.
3. **Every state in words and colour.** Every error says what happened and what to do next; nothing spins forever.
4. **Empty states teach**, with the action that fills them; a project's first page is a short checklist.
5. **Developer settings live under Settings**, out of an editor's way, and are hidden from roles without permission.
6. **Confirm only what can't be undone**; prefer undo.
7. **Every screen works on a phone** for reading and reviewing.

## One glossary file <Status done="3.7" />

Every word the Studio shows for these ideas comes from one file, `apps/studio/src/lib/words.ts`. A test, `apps/studio/tests/server/words.spec.ts`, fails when an internal term appears in visible text, an `aria-label`, a `title` or a placeholder on an everyday screen or empty state, outside Settings and the *Technical details* drawer; it also checks that the drawer has those terms, and that the check sees them in Settings, so it can't pass by seeing nothing. Words people wrote themselves, such as a project they named "Changesets testbed", are theirs and skipped.

| In the API and these docs | In the Studio |
|---|---|
| changeset | publish request |
| revision | version |
| slug | URL name |
| destination | website connection |
| webhook, CDN, probe | site rebuild, CDN clear, site check ("Rebuild the site", "Clear the CDN", "Check the site") |
| release, generation | Release 4, with the time it went live |

A publish request's states read *Not sent for review*, *Edited since: update it*, *Waiting for review*, *Changes asked for*, *Approved*, *Approval expired* and *Published*. Ids, hashes and API addresses moved into *Technical details* drawers. The [glossary](/resources/glossary/) on this site lists both sets of words. Routes and test ids keep their internal names.

## From signing up to a first release <Status done="3.7" />

- **The front door.** Signed out, the Studio offers *Sign in* and *Create an account*, which opens the identity provider's own sign-up form (`/auth/login?start=create`). A new account grants nothing by itself.
- **Setting up a fresh installation.** While nobody is an administrator, the server prints a one-time setup code to its terminal when it starts, and a signed-in person sees *Finish setting up Colophon*: a field for the code and, in *Technical details*, the `colophon admin bootstrap` command with their own subject. Entering the code makes them the first administrator (below).
- **Where to go next.** An administrator with no project gets *Create your first project*; anyone else learns that projects are joined by invitation, with the name to give and the role to ask for. The invitation page offers an account too, so an invited person can sign up from the link.
- **Starting points.** A new project starts as a small blog (Articles get a required URL name made from the title), a documentation site (Pages with a title, a URL name and a body, beside Articles) or blank. They are applied right after the project is made, as its owner, through the same content-type API anyone could call; if that part fails, the project exists and a message says how to finish it in Settings.
- **The first-run checklist** on a project's overview: write the first article (or page), publish it (or ask for review, when your role can't approve and publish), connect the website, invite a teammate, and see the first receipt. Each item is worked out from what really exists (entries, publish requests, releases, website connections, members and invitations), and items your role can't do are left out. Two things the server can't know stay in your browser: whether you hid the list, and whether you have opened a receipt.
- **Empty states that teach**, each with the one action that fills them: articles, publish requests, releases, members (only you), website connections, pending invitations, and the overview's recent edits.

## One-click publishing <Status done="3.7" />

An entry has a *Publish* button. One click makes a publish request for the saved version and sends it for review; when the project lets people approve their own requests and your role may approve and publish in that collection, the same click approves it, recorded as your review, and publishes it on the release the page has seen. Otherwise the button is *Ask for review*, and it stops after sending it.

Underneath, nothing is skipped. It is the same [publish request](/concepts/publish-requests/), the same recorded review and the same [release](/concepts/releases-and-generations/), through the same API and the same permission checks (invariant 5), so the button can't do anything your role couldn't do in three steps. If another release went live first, it stops, says so, and links to the request. `GET /api/v1/projects/{project}` says whether people may approve their own requests (`self_review`), and the button follows it; today it is always true, because the rule that requires a different reviewer (D-044) is off until projects have settings.

## Developer settings, out of the way <Status done="3.7" />

Content types, website connections and roles live under Settings, and each part is offered only to roles with its permission (`schema.change`, `destinations.manage`, `roles.manage`), checked against the caller's permissions rather than role names ([Roles and permissions](/concepts/roles-and-permissions/)). The server still decides every request. API tokens arrive with R.4. <Status planned="R.4" />

## States and errors in words

The receipt labels speak to editors: *Published in CMS*, *Website update pending*, *Verified at 1 location* and *Needs attention* ([Receipts and probes](/concepts/receipts-and-probes/)). <Status done="3.3" /> Step 3.7 rewrote the refusals editors meet in the same spirit: a URL name another entry has says which entry has it, and a new entry made from a title that is taken gets the next free URL name (`hello-readers-2`). It also fixed a comparison that, opened directly, could say "Loading the comparison…" for good.

## On a phone

Rule 7 asks that every screen work on a phone for reading and reviewing. So far the journey test opens two screens at 390 pixels wide, the publish request list and one publish request, and checks that neither scrolls sideways and that the comparison loads. The other screens aren't checked at phone width yet, which is the part of step 3.7 still open. <Status planned="3.7" />

## The five-editor usability gate

Spec §05 sets a usability gate: five representative editors each try to draft, preview, accept one AI edit and publish, and the proposed target is that at least four finish without a developer's help, with every failure recorded and the design revised. The spec calls it an experiment, not a claim already achieved, and so does Colophon: it hasn't been run. D-059 makes each milestone end with the five-editor test and its failures written up, and the roadmap puts the full test in P6, before v0.1. The AI step waits for an assistant, which is optional and off by default. <Status planned="P5" />

## The first administrator <Status done="3.7" />

Today the first administrator can be made from the browser, as well as with `colophon admin bootstrap` from a terminal (decision D-061, amending D-043). While nobody is an administrator, a server that serves the API issues a setup code when it starts and prints it once to its standard error, never to the structured log. A signed-in person who sends it to `POST /api/v1/setup` becomes the first administrator, recorded exactly as the bootstrap command records it.

- The code is 20 characters of Crockford's base-32 (100 random bits), valid for one hour, used once, and stored only as a SHA-256 hash (migration 0020); the server hashes it as soon as the request is read and compares it with every live code in constant time.
- Wrong, used and expired codes get one answer, so a guesser learns nothing (`422 setup_code_invalid`). Attempts are limited in PostgreSQL, so every server instance shares the count: five a person and twenty in all per fifteen minutes, then `429 rate_limited` with `Retry-After`.
- Once anyone is an administrator, every part of it refuses (`409 already_set_up`) and later starts print nothing. An operator who wants no code ever issued runs the bootstrap command before the first start.
- The application's database role can't read or write the tables involved; four `SECURITY DEFINER` functions are its only way in, and they check all of the above themselves.
- A code printed to standard error can sit in collected container logs for its hour; it dies at first use and whenever anyone becomes an administrator.

## Onboarding and the project lifecycle

The spec's onboarding contract (§04) covers the operator, the editor and a project's whole life. Where it stands:

| Spec §04 asks | Where it stands |
|---|---|
| A one-time, local bootstrap binds the first owner to an issuer and subject, then is disabled | `colophon admin bootstrap` (step 1.8) <Status done="1.8" />, and a setup code entered in the browser (step 3.7) <Status done="3.7" /> |
| A readiness report per dependency | `colophon doctor` <Status done="P0" /> |
| An invitation is single-use, expires (72 hours by default) and binds to the identity that accepts it | Step 1.7 <Status done="1.7" /> |
| The invitee sees the project, the role and its capabilities before accepting | The project, the role's name and description, and who invited them <Status done="R.2" /> |
| The first screen offers the editor's next actions, never database or cache settings | The first-run checklist <Status done="3.7" /> |
| Upload media and ask AI from the first screen | Media in P4 <Status planned="P4" />; AI in P5 <Status planned="P5" /> |
| Development, staging and production environments | Only `production` exists today <Status planned="not scheduled" /> |
| Rename, clone an environment, archive, delete (with fresh authentication and an impact preview), export | Export is in milestone D (and the CLI's in P5); the rest has no step yet <Status planned="not scheduled" /> |

## How it is tested

- `apps/studio/tests/server/journey.spec.ts` runs the whole first run on a new installation, with its own database (migrated and never bootstrapped) and its own server, whose setup code it reads from standard error: a new person signs up, enters a wrong code and then the right one, creates a project from the blog starting point, writes an article (its URL name made from the title), publishes it in one click, opens its receipt and invites a teammate; the teammate signs up, joins from the link as an editor and asks for review; the owner approves and publishes it; the hidden checklist stays hidden. axe finds nothing on any of its screens in light or dark.
- `apps/studio/tests/server/words.spec.ts`: the plain-words check above.
- `crates/colophon-db/tests/setup.rs` for every property of the setup code, and `crates/colophon-server/tests/setup.rs` against the real binary on a database nobody set up, where the code appears exactly once in all the output; `crates/colophon-server/tests/sign_in.rs` makes an account on Keycloak's sign-up form.
- `apps/testbed/tests/setup.spec.ts`: the testbed's Setup page through the real API. Signed out is `401`; on a set-up installation every code is `409 already_set_up`, again and again.
- `perf/k6/setup.js`, on an installation that already has its administrator: nine requests in twenty present a setup code and must get `409`, the rest read `/me`, and teardown checks nobody became an administrator. On the maintainer's laptop on 27 October 2026: 1,000,012 requests at 3,099 a second, p99 36.2 ms, and a steady run at 2,000 a second with a p99 of 58.1 ms. Two first runs on an oversubscribed laptop are kept too, one of them missed ([performance](/resources/performance/)). It isn't among CI's load smokes.

`scripts/first-run.sh` starts a fresh, never set-up installation on port 58400, for trying the journey by hand.

## Design

Decisions D-059 and D-061 in the [decisions](/design/decisions/). Step 3.7 is in the [roadmap](/resources/roadmap/) as "the non-technical path". The Studio's design is in [module 11, studio](/design/modules/11-studio/) and [module 14, ui](/design/modules/14-ui/); the setup code in [module 03, auth](/design/modules/03-auth/).
