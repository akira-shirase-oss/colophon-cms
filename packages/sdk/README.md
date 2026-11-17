# @colophon-cms/sdk

The TypeScript SDK for [Colophon CMS](https://github.com/akira-shirase-oss/colophon-cms): every
type generated from the OpenAPI document the server itself serves, a small hand-written client,
typed errors, and generation-pinned delivery reads.

```ts
import { Client, newIdempotencyKey } from '@colophon-cms/sdk';

const client = new Client({ baseUrl: 'http://localhost:58000', token });
const env = client.environment(projectId); // production by default

const { data: draft, response } = await env.getEntry('articles', entryId);
await env.saveEntry('articles', entryId, { ...draft.content, title: 'New title' }, {
  ifMatch: response.etag!,            // the ETag the read carried
  idempotencyKey: newIdempotencyKey() // one key, reused if you retry this save
});
```

## Read published content and render it (five minutes)

```sh
npm i @colophon-cms/sdk @colophon-cms/rich-text
```

```ts
import { Client } from '@colophon-cms/sdk';
import { renderToHtml, type RichTextValue } from '@colophon-cms/rich-text';

const origin = 'https://cms.example.com';
// Anonymous: published content only, which is all a site needs.
const site = new Client({ baseUrl: origin }).delivery('my-site'); // production by default

const page = await site.list('articles', { limit: 10 });
for (const entry of page.data) {
  const body = entry.content?.['body'] as RichTextValue | undefined;
  const html = body === undefined ? '' : renderToHtml(body, {
    assetUrl: (assetId) => `${origin}/media/v1/my-site/production/${assetId}`
  });
  console.log(entry.title, html.length);
}

// The rest of this page's reads come from the same release, even if another lands meanwhile.
const release = page.response.generation;
const pinned = release === null ? site : site.atGeneration(release);
if (page.next_cursor) await pinned.list('articles', { limit: 10, cursor: page.next_cursor });
```

**Never ship a staff or personal token to a browser.** Delivery reads need no credential; when a
server-side script needs more, give it a project API token (`colophon_api_…`) with the least
role that works, and keep it in that server's environment.

## Is this SDK right for that server?

```ts
import { Client } from '@colophon-cms/sdk';

const client = new Client({ baseUrl: 'https://cms.example.com', compat: 'throw' });
const fit = await client.compatibility();
if (!fit.compatible) console.error(fit.reason); // which side to upgrade, and to what
```

The SDK checks once, before its first API call: by default (`compat: 'warn'`) it says why
through `console.warn` and never delays or fails a call; `'throw'` fails every call with an
`incompatible_server` `ApiError` while the server doesn't fit; `'off'` never asks. The rule is
the CLI's (D-062): the server must serve API `v1`, share the SDK's minor version before 1.0,
and be at least the oldest server the SDK supports.

## What it covers

The client wraps the core surface — about a third of the API's 104 operations, chosen for what a
site, tool or agent actually calls — and keeps wire names verbatim (`next_cursor`, not
`nextCursor`):

- **server:** `livez`, `readyz`, `version`, `me`, `listProjects`, `getProject`,
  `validateRichText`
- **entries:** `listEntries` (search, `created_by`, `locale`, cursor), `getEntry`, `createEntry`,
  `saveEntry`, `deleteEntry`, `queryEntries` (the bounded query document), `entryHistory`,
  `getRevision`
- **content types:** `listContentTypes`, `getContentType`, `createContentType`,
  `updateContentType`, `deleteContentType`
- **changesets:** `listChangesets`, `getChangeset`, `createChangeset`, `submitChangeset`,
  `reviewChangeset`, `publishChangeset`
- **releases:** `listReleases`, `getReceipt` (the publish receipt)
- **comments & tasks:** list, get, create; `resolveComment`/`reopenComment`; `updateTask`
- **migrations, read only:** `listMigrations`, `getMigration`, `listMigrationItems` — planning,
  approving and applying a migration stays a person's CLI act
- **assets:** `listAssets`, `getAsset` (byte serving is `/media/v1`, deliberately not wrapped)
- **`ownPermissions`** — the caller's role as data
- **delivery:** `client.delivery(project)` — `list`, `get`, `query` over published content;
  `.atGeneration(g)` pins every read to one release so a page can never mix two

The rest of the surface (invitations, members, roles, tokens, destinations, schedules, exports,
the `/auth` flows, media bytes, the SSE event stream) is reachable the same way if you need it:
the generated `paths`/`operations`/`components` types are exported, so extending the client is a
typed call, not a new convention.

## Errors

Every failure is an `ApiError`: the HTTP `status` (0 when no answer arrived), the envelope's
`code` and `message`, `requestId`, `details` (`reason`, `pointer`, entries in the way, the
missing permission), the conflict's current `etag`, `retryAfterMs` on a 429, and a `retryable`
flag that is true only when nothing was written (no answer, rate limit, deadline, 5xx). A retried
write must reuse its `idempotencyKey` — the client never retries silently, so a key always means
what the caller meant.

## Credentials — this client never signs in

`new Client({ baseUrl, token })` takes a credential and attaches it; minting one is not this
package's job (one API, no bypass). Pass a personal access token from `colophon login`, a project
API token (`colophon_api_…`), or a session descriptor:
`session: { csrf, cookie? }` — the readable `colophon_csrf` value echoed on writes, plus the
`colophon_session` cookie's value **outside a browser only** (in a browser the cookie travels by
itself and can't be set as a header; cookie-authed writes from non-browser contexts also need an
`Origin` the server trusts — `headers: { origin }`).

## Bounded everything

`timeoutMs` (default 15 s) bounds every request and joins your `signal`; `maxBodyBytes` (default
16 MiB) stops a body read early — declared or measured — as `response_too_large`.

## The types never drift

`src/api.types.ts` is generated by `openapi-typescript` from `docs/api/openapi.json` — itself a
byte-compared artifact of the route table the router is built from (D-110). Regenerate after the
document changes:

```sh
npm run gen -w @colophon-cms/sdk      # rewrites src/api.types.ts
npm run check -w @colophon-cms/sdk    # tsc --noEmit + fails when the types are stale
```

The `--check` half runs in `scripts/frontend.sh`, so CI refuses a stale file exactly as the Rust
test refuses a stale `openapi.json`.

## Packaging

ESM only, no runtime dependencies, Node 22.17+ or any browser. `npm run pack-check -w
@colophon-cms/sdk` packs this package and `@colophon-cms/rich-text` and checks what npm would
ship: the file allowlist and size budget, publint, Are the Types Wrong, and a scratch project
that installs the tarballs, calls a stub server, renders every rich-text fixture, compiles this
README's examples under `nodenext` and `bundler` resolution, and bundles the client for a
browser (12 KB min+gzip budget).

## Layout

- `src/api.types.ts` — generated (do not edit; `openapi-typescript@7.13.0`,
  `--empty-objects-unknown`)
- `src/client.ts` — `Client`, `Environment`, `Delivery`, `newIdempotencyKey`
- `src/compat.ts` — the server-version rule (`SDK_VERSION`, `compatibility()`'s verdicts)
- `src/error.ts` — `ApiError`, the envelope types
- `src/index.ts` — exports and the common schema aliases
- `test/` — mocked-fetch contract tests, type-level tests, the compatibility table, and a
  real-server suite gated on `COLOPHON_TEST_ORIGIN`
- `scripts/pack-check.mjs` — the packaging gate above

Build emits `dist/` (`npm run build`, NodeNext ESM + `.d.ts`). The release workflow packs,
checks and publishes it with provenance (under the `next` tag while the version is a
pre-release) once a maintainer approves; nothing is published from a laptop. The package is
versioned with the server.
