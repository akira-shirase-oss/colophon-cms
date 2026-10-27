<script>
	import Status from '#lib/components/Status.svelte';
</script>

**Being built. Nothing on this page works yet.** Step 3.7 is being built on the branch `p3/3.7-onboarding`, which isn't merged. What follows is decision D-059 and the parts of the master spec it makes binding; today's Studio still uses the old words and has no checklist or one-click publishing. The last section says what the branch has so far.

Colophon's goal includes people who aren't developers running it without help: writing, getting a review and publishing, without learning what a changeset or a destination is. Decision D-059 (27 October 2026, the maintainer's requirement) turns that into rules every Studio screen follows, and makes the spec's usability gate (§05) binding on every step.

## Why it is needed

The spec already says the Studio must not expose internal job terminology where "Website update pending" is enough, and must keep technical detail in a drawer for developers. When D-059 was written, the Studio still showed "changeset" 15 times, and "destination", "probe", "webhook" and "pinned" on everyday pages. The engine underneath is not the problem; the words and the number of steps are.

## The seven rules

D-059's rules for every Studio screen:

1. **Plain words, from one glossary.** A changeset is a *publish request*, a revision a *version*, a release keeps its name with the time it went live, a slug is a *URL name*, and destinations are *website connections*: rebuild the site, clear the CDN, check the site. Internal terms stay in the API, the command line and a *Technical details* drawer.
2. **One primary action per screen** (the black pill button), and a guided path: write, review, publish.
3. **Every state in words and colour.** Every error says what happened and what to do next; nothing spins forever.
4. **Empty states teach**, with the action that fills them; a project's first page is a short checklist.
5. **Developer settings live under Settings**, out of an editor's way, and are hidden from roles without permission.
6. **Confirm only what can't be undone**; prefer undo.
7. **Every screen works on a phone** for reading and reviewing.

## One glossary file

Every word the Studio shows will come from one file in the Studio, `lib/words.ts`, and a test fails if an internal term appears in visible text outside Settings and the technical drawer. Step 3.7 applies it to every existing screen; every later step's Studio work is reviewed against the seven rules.

| In the API and these docs | In the Studio |
|---|---|
| changeset | publish request |
| revision | version |
| slug | URL name |
| destination | website connection |
| webhook, CDN, probe | rebuild the site, clear the CDN, check the site |
| release, generation | the release's name and the time it went live |

The [glossary](/resources/glossary/) on this site lists both sets of words. Until 3.7 lands, the Studio shows the left-hand column in places.

## One-click publishing

Publishing one entry becomes one button on the entry. It creates the publish request, and then publishes it at once when the project lets people review their own work (the spec's default, §06), or asks for review otherwise.

Underneath, nothing is skipped. It is the same [publish request](/concepts/publish-requests/), the same recorded review (marked as a self-review) and the same [release](/concepts/releases-and-generations/), through the same API and the same permission checks (invariant 5), so the button can't do anything the person's role couldn't do in three steps. Today every project allows self-review: the rule that requires a different reviewer exists in the domain and is off until project settings exist.

## The first-run checklist and empty states

A project's first page is a short checklist: **write** something, **invite** someone, **connect the website**, **publish**. Every empty page teaches what belongs there, with the action that fills it.

## Developer settings, out of the way

Content types, website connections and API tokens live under Settings, and a role without the permission doesn't see them. Part of this exists on the R.2 branch already: Settings holds the content-type builder (step 3.6), roles (step R.2) and, for people whose role may manage them, website connections, and the Studio offers controls by checking the caller's permissions rather than role names ([Roles and permissions](/concepts/roles-and-permissions/)). <Status building="R.2" /> API tokens arrive with R.4. <Status planned="R.4" />

## States and errors in words

The receipt labels already speak to editors: *Published in CMS*, *Website update pending*, *Verified at 1 location* and *Needs attention* ([Receipts and probes](/concepts/receipts-and-probes/)). <Status done="3.3" /> Step 3.7 holds every other state and error to the same rule: what happened, and what to do next.

## The five-editor usability gate

Spec §05 sets a usability gate: five representative editors each try to draft, preview, accept one AI edit and publish, and the proposed target is that at least four finish without a developer's help, with every failure recorded and the design revised. The spec calls it an experiment, not a claim already achieved, and so does Colophon: it hasn't been run. D-059 makes each milestone end with the five-editor test and its failures written up, and the roadmap puts the full test in P6, before v0.1. The AI step waits for an assistant, which is optional and off by default. <Status planned="P5" />

## Onboarding and the project lifecycle

The spec's onboarding contract (§04) covers the operator, the editor and a project's whole life. How far Colophon gets, step 3.7 included:

| Spec §04 asks | Where it stands |
|---|---|
| A one-time, local bootstrap binds the first owner to an issuer and subject, then is disabled | Done: `colophon admin bootstrap` (step 1.8) <Status done="1.8" />; a setup code entered in the browser on the 3.7 branch <Status building="3.7" /> |
| A readiness report per dependency | Done: `colophon doctor` <Status done="P0" /> |
| An invitation is single-use, expires (72 hours by default) and binds to the identity that accepts it | Done (step 1.7) <Status done="1.7" /> |
| The invitee sees the project, the role and its capabilities before accepting | Project and role done; the role's description on the R.2 branch <Status building="R.2" /> |
| The first screen offers the editor's next actions, never database or cache settings | The first-run checklist <Status building="3.7" /> |
| Upload media and ask AI from the first screen | Media in P4 <Status planned="P4" />; AI in P5 <Status planned="P5" /> |
| Development, staging and production environments | Only `production` exists today <Status planned="not scheduled" /> |
| Rename, clone an environment, archive, delete (with fresh authentication and an impact preview), export | Not in 3.7; export is in milestone D (and the CLI's in P5), the rest has no step yet <Status planned="not scheduled" /> |

## What the branch has so far (2026-10-27) <Status building="3.7" />

Two commits on `p3/3.7-onboarding`, made on 27 October 2026 and not pushed or merged. They start with the operator's first run rather than the editor's words: none of the seven rules above is applied yet.

- **A setup code for the first administrator.** Today the first administrator can only be made by someone holding the database owner's credentials, from a terminal. On the branch, while nobody is an administrator, the server prints a one-time setup code to its standard error when it starts (never to the structured log). A signed-in person who sends it to `POST /api/v1/setup` becomes the first administrator, recorded exactly as `colophon admin bootstrap` records it. The code is 20 characters of Crockford's base-32, valid for an hour, used once, stored only as a SHA-256 hash (migration 0020), compared in constant time against every live code, and refused with one answer whether it is wrong, used or expired. Attempts are limited in PostgreSQL: five a person and twenty in all per fifteen minutes, then `429 rate_limited` with `Retry-After`. Once anyone is an administrator, every step refuses.
- **Sign-up.** `/me` says whether the installation has an administrator yet, and `/auth/login?start=create` opens the identity provider's own sign-up form when the provider offers one. The local Keycloak realm now allows people to register themselves; a new account grants nothing until an invitation or the setup code.
- **Tests**: `crates/colophon-db/tests/setup.rs` for every property of the code, and `crates/colophon-server/tests/setup.rs` against the real binary on a database nobody has set up, where the code appears exactly once in all the output.

The branch records this as **D-060 on `p3/3.7-onboarding`**, amending D-043; a different decision has the same number on `e/schedules`, so the number will change on one of them. The glossary file, one-click publishing, the checklist, the empty states and phone-friendly review aren't on the branch yet.

## Design

D-059 is on the R.2 branch: [DECISIONS.md](https://github.com/akira-shirase-oss/colophon-cms/blob/r/r.2-custom-roles/docs/DECISIONS.md). Step 3.7 is in the [roadmap](/resources/roadmap/) as "the non-technical path". The Studio's current design is in [module 11, studio](/design/modules/11-studio/) and [module 14, ui](/design/modules/14-ui/).
