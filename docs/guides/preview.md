# Draft preview on your own site

An editor clicks **Preview** in the Studio and sees their unpublished draft rendered by your
real site, in a pane beside the editor or in a new tab, and it refreshes after each save. Your
site needs three things: a **preview handler** that trades a one-time code for a short-lived
token, pages that read drafts with that token, and a few headers. `@colophon-cms/sdk` provides
all three on the Web `Request`/`Response` types, so they work in any framework.

## How it works

1. You set the environment's **preview target** in the Studio (owners and developers), for
   example `https://www.example.com/api/preview?code={code}&path=/blog/{id}`. The placeholders
   are `{code}` (required), `{collection}`, `{id}`, `{slug}` and `{locale}`. The address must be
   `https://`; plain `http://` is accepted only for `localhost` on a development server.
2. **Preview** issues a one-time code (`cpc_…`, valid for 60 seconds, usable once) and opens the
   address with it filled in.
3. Your handler exchanges the code with Colophon from your server for a bearer token (`cpv_…`,
   15 minutes by default, renewed while the Studio stays open, never more than 8 hours). It
   stores the token in your own `HttpOnly` cookie and redirects to `path`.
4. Your pages see the cookie and read drafts from `/preview/v1` with `Authorization: Bearer`.
   Every read checks the token again: expired or revoked tokens, issuers who lost access, and
   changesets that changed or went live are all refused, with a `reason` you can show.

## SvelteKit

```ts
// src/lib/server/preview.ts
import { createPreviewHandler, exitPreviewHandler, previewClient } from '@colophon-cms/sdk';
const cms = { baseUrl: 'https://cms.example.com', project: 'blog' };
export const STUDIO = 'https://cms.example.com';
export const preview = previewClient(cms);
export const enter = createPreviewHandler({ ...cms, isAllowedPath: (path) => path.startsWith('/blog/') });
export const exit = exitPreviewHandler();

// src/routes/api/preview/+server.ts      → export const GET = ({ request }) => enter(request);
// src/routes/api/preview/exit/+server.ts → export const GET = ({ request }) => exit(request);
```

```ts
// src/routes/blog/[id]/+page.server.ts
import { Client, previewHeaders, previewTokenFromRequest } from '@colophon-cms/sdk';
import { preview, STUDIO } from '$lib/server/preview';

export async function load({ params, request, setHeaders }) {
  const token = previewTokenFromRequest(request);
  if (token) {
    setHeaders(previewHeaders({ studioOrigin: STUDIO }));
    const { data } = await preview.reader(token).get('articles', params.id);
    return { entry: data, previewing: true };
  }
  const { data } = await new Client({ baseUrl: 'https://cms.example.com' }).delivery('blog').get('articles', params.id);
  return { entry: data, previewing: false };
}
```

```svelte
<!-- +page.svelte: tell the Studio this page is ready, reload after each save -->
<script>
  import { listenForDraftUpdates } from '@colophon-cms/sdk';
  let { data } = $props();
  $effect(() => listenForDraftUpdates({ studioOrigin: 'https://cms.example.com', previewing: data.previewing }));
</script>
```

The complete example, with a "Preview mode" banner, an exit link and handling for previews that
have ended, is the testbed's site: `apps/testbed/src/routes/preview/`. Its Playwright suite
(`apps/testbed/tests/preview.spec.ts`) runs it against the real server.

## Astro (server output)

```ts
// src/pages/api/preview.ts
import { createPreviewHandler } from '@colophon-cms/sdk';
const enter = createPreviewHandler({ baseUrl: import.meta.env.CMS_URL, project: 'blog' });
export const GET = ({ request }) => enter(request);
```

```astro
---
// src/pages/blog/[id].astro (export const prerender = false)
import { previewClient, previewHeaders, previewTokenFromRequest } from '@colophon-cms/sdk';
const token = previewTokenFromRequest(Astro.request);
if (token) {
  for (const [name, value] of Object.entries(previewHeaders({ studioOrigin: import.meta.env.CMS_URL })))
    Astro.response.headers.set(name, value);
}
const entry = token
  ? (await previewClient({ baseUrl: import.meta.env.CMS_URL, project: 'blog' }).reader(token).get('articles', Astro.params.id)).data
  : await getPublished(Astro.params.id); // your usual /content/v1 read
---
```

Call `listenForDraftUpdates` from a `<script>` on the page, as shown in the SvelteKit example.
A fully static Astro build has no server to run the handler. Give it one server-rendered route,
or render preview pages with an adapter.

## Next.js (App Router), in outline

```ts
// app/api/preview/route.ts
export const GET = createPreviewHandler({ baseUrl: process.env.CMS_URL!, project: 'blog' });
// app/blog/[id]/page.tsx: read the cookie with cookies().get('colophon_preview'), read through
// previewClient(...).reader(token), and send previewHeaders(...) from middleware for that path
// (or use the token's presence to opt the route into dynamic rendering).
```

## Keep drafts out of caches and search engines

- **The SDK's headers.** Every handler response is `Cache-Control: no-store`, `X-Robots-Tag: noindex`
  and `Referrer-Policy: no-referrer`. A page rendered in preview mode must send
  `previewHeaders({ studioOrigin })`: `Cache-Control: private, no-store`, `X-Robots-Tag: noindex`
  and `Content-Security-Policy: frame-ancestors <studio>`, so only your Studio can frame it. Colophon's
  own `/preview/v1` responses are `private, no-store` too.
- **Your CDN.** Shared caches already refuse to store `no-store`, `private` and `Set-Cookie`
  responses. Add these rules anyway:
  - **bypass the cache for the handler route** (`/api/preview*` above), and for any prefix that
    serves only preview pages (the testbed uses `/preview/*`);
  - **bypass it for any request carrying the `colophon_preview` cookie.** Most CDNs leave cookies
    out of the cache key. Without this rule, an editor whose page is already cached for the
    public sees the published copy instead of their draft;
  - if a CDN also sits in front of Colophon itself, bypass **`/preview/v1/*`** there. Those
    responses are `private, no-store`, `Vary: Authorization`, and never carry `Cache-Tag` or
    `ETag`.

  The testbed checks this with a "cache everything" proxy in front of its site: a preview stores
  nothing, anonymous visitors keep getting the published page, and with the cookie rule in place
  the editor gets the draft.
- **robots.txt.** Disallow the handler route and any preview-only prefix. Crawlers then won't
  spend codes or index the expired-link page. Your normal pages stay indexable, because preview
  mode depends on the cookie and sends `noindex` on its own:

  ```
  User-agent: *
  Disallow: /api/preview
  Disallow: /preview/
  ```

## Third-party cookies and the new-tab fallback

Inside the Studio's iframe your site is a third party, so its cookie needs `SameSite=None; Secure`.
Browsers that block third-party cookies would still drop it. The handler therefore sets a
**partitioned** cookie (`Partitioned`, also called CHIPS), which is stored separately for each
top-level site. Chromium-based browsers accept it inside the frame, and support in other browsers
varies. Colophon never needs a cookie of its own inside the frame, because every preview read is
a bearer request from your server. When the cookie still doesn't arrive, your page renders
without a preview session and `listenForDraftUpdates` reports `previewing: false`. The Studio
then says the site's cookie was blocked and offers **Open in a new tab**, where the cookie is
first-party and always works.

For local development, Chromium accepts `Secure` cookies on `http://localhost`. If your Studio
is public and your site is on `localhost`, Chrome will first ask whether the Studio page may
reach your local network (Local Network Access).

## The postMessage handshake

| From | Message | `targetOrigin` / check |
|---|---|---|
| Site page (via `listenForDraftUpdates`), on load, only when framed | `{ type: "colophon:preview-ready", previewing: true \| false }` | posted to `window.parent` with the Studio's exact origin |
| Studio, after each **acknowledged** save | `{ type: "colophon:draft-saved", entry, revision }` | posted with the grant's `origin`, the preview target's origin |

Both sides compare `event.origin` with the expected origin exactly, never with a prefix, and never
use `*`. `listenForDraftUpdates` reloads the page by default. Pass `onUpdate` to re-run your own
data loading instead (for SvelteKit, `invalidateAll()`).

## Errors you may see

Preview reads fail as `ApiError`. Use `error.reason` to decide what to show:
`preview_expired` / `preview_revoked` / `preview_invalid` (401: show the published page and say
the preview ended), `preview_stale` (409: the publish request changed, so preview it again),
`preview_published` (410: it went live, show the published page), and 503 with `retryAfterMs`
when preview capacity is full. A used, unknown or expired code makes the handler answer its small
404 page.
