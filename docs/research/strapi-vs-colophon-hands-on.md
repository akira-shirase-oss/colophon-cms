# Strapi vs Colophon Studio, hands on (2026-10-28)

The maintainer asked "have you checked how easy it is to use Strapi?". The earlier research
([cms-ui-patterns.md](cms-ui-patterns.md)) only read Strapi's and Sanity's docs. This time both products were
installed and used through their UIs, doing the same seven tasks as a non-technical person who has never seen either.
Screenshots (55 of Strapi, 49 of Colophon) and per-step logs were kept on the maintainer's machine, not committed.

## What was tested

| | Strapi | Colophon |
|:--|:--|:--|
| Version | Strapi 5.60.0 Community, Node 24.18.0, SQLite, development mode | Studio from `239e72a` (`studio/dashboard`); server `0.1.0-alpha.1` at `fe17e4f` (no Rust changes between them); fresh database |
| Install | `npx create-strapi@latest` (non-interactive): 67 s to install, ready 6 s later | The local stack (Postgres, Keycloak, Valkey, Garage) already running; a new database, migrations, server start |
| First admin | The first visitor's registration form | The real first-run path (D-061): migrated without `admin bootstrap`, so the server printed a setup code |
| Second person | Invited admin user with a custom role | `dev-editor`, invited by link with a custom role |

**Method.** Headless Chromium at 1440×900 driven by Playwright, every action through the UI (the APIs only to check
results). Counts follow the path a first-timer would actually take, including a newcomer's wrong turns (Strapi's
singular/plural and "regex" errors, its side-panel search; Colophon's Members → Settings → Roles hunt). **Times are
estimates, not stopwatch readings**: 1.5 s a click, 0.3 s a keystroke, 4 s to read each new screen, plus measured waits
(Strapi's restarts, Colophon's publish) and an allowance for each hesitation noted.

**Deviations.** Colophon's *Create an account* (Keycloak's 6-field register form) was photographed but not submitted,
so nothing was added to the shared Keycloak; its cost is estimated. Strapi's `--dbclient sqlite` without `--dbfile`
wrote an empty `DATABASE_FILENAME` and crashed; it was set to Strapi's default `.tmp/data.db` (a CLI-flag pitfall the
interactive installer avoids, not counted against the admin UI).

## Totals

| | Clicks | Keystrokes | Est. time in the UI | Errors shown | Docs needed |
|:--|--:|--:|--:|--:|:--|
| Strapi | ~123 | ~431 | ~14.5 min | 2 (API ID, "regex") | Yes, for the public API address |
| Colophon | ~98 | ~341 | ~9 min | 0 | No |

Neither total includes installation. **Strapi's install is far easier**: about 75 s with only Node, against Colophon's
Docker stack (fix 1).

## Task by task

| Task | Strapi | Colophon |
|:--|:--|:--|
| 1. First run to signed-in admin | 6 clicks / 49 keys, ~0.5 min (+75 s install). One form. | 9 / 30, ~1.2 min (~2 with Keycloak registration). The front door tells the installer to "ask the person who runs this Colophon to invite you" (that's them), then a separate identity-provider site, then a code copied from a scrolling server log |
| 2. "Products" with 5 fields | ~36 / 71, ~3.6 min. Errors: "Products" makes singular = plural (refused); "On sale" fails "The value does not match the regex."; Required on the Advanced tab; Number asks integer / big integer / decimal / float; every save restarts the server (4.9 s) | 25 / 58, ~1.7 min, 0 errors, save 0.8 s. But every type starts with a forced *Title*, so "Name" means relabelling it, and the "Title" badge stays |
| 3. Create, publish, find the public address | ~25 / 114, ~5.5 min (3 in the docs). The admin never shows the address; `/api/products` is 403 until the Public role gets `find`; `:id` is the document id, so `/api/products/2` is 404 | 12 / 82, ~1.3 min. Address and *See what readers get* under *Technical details*; the editor moved rich text below the other fields; the JSON key for "Name" is `title` |
| 4. Edit and republish | 4 / 5, ~0.3 min; the badge still says "Published" while unsaved | 4 / 5, ~0.3 min; "Changed — readers see Release 1, not your newer changes" |
| 5. Single "Homepage" | 18 / 109, ~1.4 min; opens the form directly | 19 / 109, ~1.5 min; the forced Title again, and a one-entry type opens as a list with tabs and "Search titles" |
| 6. A writer who can't publish | 29 / 74, ~2.6 min; two "Roles" pages; the writer's Publish is grey with no reason | 28 / 52, ~2.8 min; no route from Members to Roles; "Change fields" and "Approve" unexplained; *Limited permissions* shows raw keys; after *Join*, a near-empty card, and reloading says "This link has no invitation in it" |
| 7. Search for "tote" | 5 / 9, ~0.6 min; the most visible search box filters content types, not entries | 1 / 5, ~0.1 min; ⌘K search over every entry |

## Where Strapi is easier, ranked, each with a Colophon fix

1. **Zero to "I'm the admin".**
   - **S:** on an installation with no administrator, the signed-out front door says "This Colophon is new. Create
     the first account; you'll become its administrator with the setup code the server printed", with *Create an
     account* as the primary button.
   - **S:** the server prints a one-click link beside the code (`…/studio/#setup=CODE`, a fragment, so it never
     reaches a log), and *Finish setting up Colophon* fills itself in from it.
   - **L:** a one-command local start (`colophon quickstart`: stack up, migrate, start, open that link).
2. **The forced "Title" field (M).** Ask "What is each one called?" when creating a type and use it as the title
   field's label and key. Everywhere else show that label (list column, "Search names", the preview heading,
   *Limited permissions*). Rename the badge "Shown in lists".
3. **No route from Members to Roles (S).** "Make a new role…" as the last option of the invite dialog's Role select
   (returns with it chosen), and a "Manage roles" link beside it.
4. **The role grid's words (S).**
   - "Change fields" becomes "Change its fields", with help: "Add, rename or remove this type's fields (developers)".
   - One line of help under See, Approve and Publish.
   - *Limited permissions* lists the chosen type's field labels.
5. **Joining from an invitation ends in a dead end (S).** After *Join*, go to the project's *Your work* page with a
   toast. For a signed-in member, a tokenless `/studio/invite` says "Nothing to join here" and offers *Open your
   projects*.
6. **The list hides the fields that matter (S; a column picker is M).** Show up to three short fields (number, choice,
   yes/no, date) as columns by default.
7. **One-entry types open as a list (S–M).** The sidebar opens the editor directly, creating the entry on first open.
   "See its entries" becomes "Open it".
8. **The editor reorders fields (S).** `EntryEditor.svelte` renders plain fields, then rich text. Follow the builder's
   order instead.
9. **Get started doesn't fit your content (S).**
   - *Start blank* still creates Articles; make it really blank.
   - Lead with "Decide what you'll write (Content types)".
   - Take the list's noun from the person's first collection.

Unranked: a visible "Copy public link" on live entries; "changeset" left in the Technical details sentence; "CDN" and
"Published in CMS" on the receipt page (each S).

## What Colophon does better

- **Plain words.** "Number: a price, a quantity, a rating", "Yes or no", "Choice". Strapi has Boolean, Enumeration
  and "Rich text (Blocks): the new JSON-based…".
- **No API ids and no regex errors.** Labels with spaces just work, and Required sits on the first tab.
- **No restart.** Type changes save in 0.8 s, where Strapi restarts on every save, and its builder works only in
  development mode.
- **A clear unsaved-changes bar.**
- **The public address on the entry.** It's public by default, with a copy button and the exact JSON readers get.
- **Autosave and an honest "Changed" state.**
- **A writer who can't publish is told what to do** (*Ask for review*), with a scoped sidebar and a *Your work* home.
- **One search for every entry.**
- **A labelled sidebar** where Strapi has an icon rail.
- **No modal tours.** Strapi's tours (about 18 popovers) blocked clicks and once covered an open field dialog.
- **Nothing in the seven tasks was impossible in Colophon, or needed documentation.**

Not rated, being features one side lacks rather than ease of use: Strapi's Media Library, locales and Marketplace;
Colophon's releases, receipts and site checks; Strapi Community's lack of review requests.
