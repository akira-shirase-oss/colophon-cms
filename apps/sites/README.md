# The five demo sites

Five independent Astro sites, each a different content category, all reading the published
content of one `demo-sites` project through Colophon's anonymous delivery API
(`/content/v1/{project}/{environment}/{collection}`) and media origin (`/media/v1/…`). They are
the usability surface for the draft → review → publish → site story: an editor changes an entry
in the Studio, publishes a release, rebuilds a site, and sees the change on a real page (D-133).

| Site | Package | Port | Collection | What it exercises |
|---|---|---|---|---|
| The Draft, a blog | `@colophon/site-blog` | 58110 | `posts` + `articles` | rich-text bodies, a slug field, tags, dates, author, a second collection beside the posts (D-138) — and a members area gated by audience sign-in (D-137) |
| Fieldnotes Docs | `@colophon/site-docs` | 58111 | `guides` | enum sections, ordered sidebar navigation, rich-text |
| Butter & Thyme, recipes | `@colophon/site-recipes` | 58112 | `recipes` | a list of objects (ingredients), a list of text (steps), enums, numbers |
| The Meridian Post, news | `@colophon/site-news` | 58113 | `stories` | a featured flag, categories, datelines, bylines |
| Wren Hollis, portfolio | `@colophon/site-portfolio` | 58114 | `photos` | real uploaded assets served from `/media/v1`, renditions `w320`/`w1280` |

One project, five collections: the point is that the same published projection can feed front
ends that share nothing but the API.

## Seeding

With the local stack and `colophon-server` running (see `deploy/README.md`):

```sh
scripts/seed-demo-sites.sh            # against http://localhost:58000
scripts/seed-demo-sites.sh <origin>   # against another server
```

The seed signs in as the development realm's `dev-owner`, creates a `demo-sites-<stamp>`
project and the five content types, draws ten PNG photographs and pushes them through the real
upload → quarantine → verify pipeline, writes about fifty entries, then submits, approves and
publishes them all as release 1 of one changeset. Nothing it writes is a draft: every site
reads published content only. Three of the blog's posts are published gated — two `role:member`,
one `authenticated` — and the seed creates the audience roles they name plus a demo member
account, `member@draft.test` / `member-of-the-draft`. Finally it writes
`apps/sites/.colophon.json` (the project id, the API origin and the release generation), which
the sites read at build time. The file is gitignored, and every run makes a new project, so
re-seeding is safe.

## Building and serving

```sh
scripts/serve-demo-sites.sh           # build all five, then serve them (Ctrl-C stops)
scripts/serve-demo-sites.sh build     # build only
scripts/serve-demo-sites.sh serve     # serve what is already built, in the background
scripts/serve-demo-sites.sh stop      # stop the background sites
scripts/serve-demo-sites.sh blog      # build and serve one site in the foreground
```

`npm run build -w @colophon/site-blog` (or any of the five packages) builds a site by hand; it
reads `apps/sites/.colophon.json`, or the `COLOPHON_API`, `COLOPHON_PROJECT` and
`COLOPHON_ENVIRONMENT` variables when set. With neither, the build still succeeds and draws a
page explaining the seed, so a clean checkout and CI never fail.

## Publishing rebuilds them (the website connection)

`scripts/demo-sites-hook.mjs` closes the loop between publishing and the sites (D-138): it
listens on `http://127.0.0.1:58120/hook`, registers itself as the project's **website
connection** (a `webhook` destination, through the real API), and rebuilds all five sites when
a release announcement arrives.

```sh
node scripts/demo-sites-hook.mjs      # signs in, ensures the connection, listens, rebuilds
curl -s http://127.0.0.1:58120/healthz  # what it has built, and the last failure if any
```

Publishing in the Studio then updates every site without a person running anything: the worker
posts one announcement per release, the hook answers `202` at once, builds the newest announced
generation (`serve-demo-sites.sh build` then `serve`), and duplicate announcements cost
nothing. A failed build leaves the running sites as they were and is retried after a pause.
`HOOK_PORT`, `COLOPHON_SERVER`, `COLOPHON_ADMIN_USER`/`COLOPHON_ADMIN_PASSWORD` and
`HOOK_BUILD_ON_START=0` tune it.

## How a site is put together

Every site is a plain, fully static Astro app — except The Draft's members pages (below). At
build time a site pages through its collection's published entries (the cursor carries the
first page's generation, so a build never mixes two releases), renders them, and writes the
release number into every page as `<meta name="colophon-generation" content="N">`, the marker
probes read. Rich-text fields go through `@colophon/content-renderer`, the one renderer the
Studio's preview and the reference site share. The only shared code is `@colophon/site-kit`
(`apps/sites/shared`): the config, the delivery reads — anonymous or with an audience bearer
token — the audience client and the render wrapper, together about two hundred lines.

A site that cannot reach its API fails the build rather than going live silently empty; a site
built before any seeding draws the "run the seed" page instead.

## The Draft's members area

The Draft also demonstrates audience authentication (docs/modules/03-auth.md, D-137): the
project's *visitors* — not staff — can sign up, sign in and read gated posts. It works like a
real site, not an API console:

- `/signin/` is a plain HTML form. It posts to the site's own `/api/signin` and `/api/signup`
  routes, which call `POST /audience/v1/{project}/signin|signup` server-side.
- The site keeps the returned session token in its own first-party cookie (`draft_session`,
  `HttpOnly`, `SameSite=Lax`) and sends it to the CMS as `Authorization: Bearer` — the CMS's
  audience cookies stay on its origin, so the site never has to forward a CSRF token.
- `/members/` and `/members/posts/{id}/` are SSR pages (`prerender = false`): each asks
  `GET /audience/v1/…/me` who the session is, then lists or reads delivery *as that member*.
  The delivery layer sees `role:member`, so Postgres row-level security answers the gated
  posts too — they are `private` answers that never touch the anonymous cache. An unsigned
  reader of the same URLs gets redirected to `/signin/`; signed-in but wrong-role members get
  a 404, the same as an unpublished entry.
- The other four sites stay fully static; gated content could never appear on them anyway,
  because anonymous delivery only ever returns `public` rows.

The blog therefore builds under Astro's node adapter: `npm run build` writes both the static
pages and `dist/server/entry.mjs`, which `serve-demo-sites.sh` runs with `HOST`/`PORT` set (the
other four still use `astro preview`). Sign in at `http://127.0.0.1:58110/signin/` with the
seeded `member@draft.test` / `member-of-the-draft`, or make a fresh account — signup takes the
open `member` role automatically.

## Republishing

Edit anything in the Studio's `demo-sites` project, run it through a changeset to a release,
then rebuild a site (`scripts/serve-demo-sites.sh <site>`). The rebuilt pages name the new
generation, which is the whole point of the marker: a page can always say exactly which release
it was built from.
