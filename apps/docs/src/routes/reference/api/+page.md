<script>
	import Status from '#lib/components/Status.svelte';
</script>

The management API lives under `/api/v1` on the Colophon server, beside sign-in (`/auth/*`), `/me`, the health checks and the public [Delivery API](/reference/delivery-api/) (`/content/v1`). The Studio, the command line and scripts all use it, with the same checks: there is no private path. This page holds the rules every route shares; [All routes](/reference/api/routes/) lists the routes, and each area page documents them one by one. Everything here is read from `crates/colophon-api/src/`.

## Addresses

One process answers everything on one port (`COLOPHON_LISTEN_ADDR`). Its public origin is `COLOPHON_PUBLIC_URL`, `http://localhost:58000` by default; invitation links and the device sign-in link are built from it.

| Prefix | What |
|---|---|
| `/api/v1` | The management API: projects, members, roles, content types, entries, publish requests, releases, website connections. |
| `/auth`, `/me` | Sign-in, sessions and device sign-in ([Sign-in and sessions](/reference/api/auth/)). |
| `/livez`, `/readyz` | Health ([Health checks](/operations/health-checks/)). |
| `/content/v1` | Public, read-only delivery ([Delivery API](/reference/delivery-api/)). |
| `/studio/` | The Studio's files, when `COLOPHON_STUDIO_DIR` is set. |

Most routes sit under a project and an environment. The reference pages write `{P}` for that prefix:

```text
{P} = /api/v1/projects/{project}/environments/{environment}
```

`{project}` is the project's id (a UUID). `{environment}` is the environment's slug; every project starts with `production`, and creating others isn't built yet. Members, invitations and roles belong to the project as a whole, so their routes have no environment.

A project you aren't a member of answers `404`, exactly as one that doesn't exist, and so does an environment, entry, changeset or other id that doesn't exist or isn't a UUID. The database enforces this with row-level security, not only the handlers ([Roles and permissions](/concepts/roles-and-permissions/)).

## JSON in, JSON out

- **Requests** with a body send `Content-Type: application/json`; anything else is `415` `unsupported_media_type`. A body that isn't valid JSON is `400` `malformed_json`.
- **Bodies with a fixed shape refuse unknown fields.** A misspelt or extra field in, say, `{"title", "entries"}` is `400` `malformed_json`, never ignored. Entry content and content-type definitions are checked against a schema instead: an unknown field there is `422` `validation_failed` with `details.reason` `unknown_field` and a pointer to it.
- **One resource** answers `{"data": …}`; **a list** answers `{"data": [...], "next_cursor": "…" or null}`. Exceptions: a version history is `{"data": [...]}` with no `next_cursor`; `POST /api/v1/rich-text/validate` answers its verdict without `data`; the health checks answer their own small objects.
- Changes that have nothing to return answer `204 No Content`: signing out, approving a device, changing or removing a member, revoking an invitation, deleting a role, turning a website connection on or off.
- Times are RFC 3339 in UTC (`2026-10-27T09:14:03.512841Z`). Content and changeset hashes are `sha256:` and 64 lowercase hex digits. Ids are UUIDs, version 7 for anything Colophon creates, so they sort by creation time.
- **Errors** are always the [error envelope](/reference/api/errors/), including errors from the middleware (a wrong method, an oversized body, a timeout, a panic): `{"error": {"code", "message", "details"?, "request_id"}}`.

## Authentication

| Client | How it signs in | What it sends |
|---|---|---|
| A browser (the Studio) | `GET /auth/login`, OpenID Connect through the configured provider | The `colophon_session` cookie. Every write also needs an exact `Origin` and an `X-CSRF-Token` header. |
| The command line, scripts | Device sign-in: `POST /auth/device`, approved by a signed-in person in the Studio | `Authorization: Bearer` and the 43-character token. No CSRF token, no `Origin`. |

**Cookies.** Sign-in sets two: `colophon_session` (`HttpOnly; Secure; SameSite=Lax; Path=/`), which holds a random token whose SHA-256 is all the database keeps, and `colophon_csrf` (`Secure; SameSite=Strict; Path=/`, readable by the page). Both last as long as the session: 12 hours after sign-in, ending sooner after 2 hours unused. Revoking a session takes effect on the very next request. Its last use is written at most once a minute.

**Writes from a browser.** Every cookie-authenticated request with a method other than `GET`, `HEAD` or `OPTIONS` needs both:

- an `Origin` header equal, character for character, to a trusted origin: the server's public origin, or one listed in `COLOPHON_TRUSTED_ORIGINS` (comma-separated). Otherwise `403` `forbidden` with `details.reason` `origin_not_allowed`, "this origin may not make changes";
- an `X-CSRF-Token` header equal to the `colophon_csrf` cookie's value. Otherwise `403` `forbidden` with `details.reason` `csrf_token_invalid`, "missing or invalid CSRF token".

```http
PUT /api/v1/projects/0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f/environments/production/collections/articles/entries/0193f6b0-2c4d-7e5f-8a91-b2c3d4e5f607 HTTP/1.1
Cookie: colophon_session=…; colophon_csrf=Jq3…
Origin: https://cms.example.com
X-CSRF-Token: Jq3…
Content-Type: application/json
If-Match: "0193f6b1-7d8e-7f90-a1b2-c3d4e5f60718"
Idempotency-Key: 4d1f6a0e-studio-save-0003
```

**Bearer tokens.** The command line gets a personal token by [device sign-in](/reference/api/auth/#post-authdevice); it lasts 30 days and `colophon logout` revokes it (`POST /auth/logout`). A request with an `Authorization` header is judged by that header alone: an invalid or expired token is `401`, never a fall back to a cookie. A token can't approve another device sign-in, so a token can't mint more tokens.

There are no API tokens for scripts and agents yet; a token is always a person's. <Status planned="R.4" />

**Who may do what** is decided per project by the caller's role, a list of permissions such as `content.update` on one collection or on all of them, which may be narrowed to some fields or to the caller's own entries ([Roles](/reference/api/roles/)). Each route below names the permission it needs and the built-in roles that hold it; [Roles](/reference/api/roles/#built-in-roles) has the full table. A caller without the permission gets `403` `forbidden`, with `details.reason` `missing_permission` and the permission in `details.permission`, but only inside a project they can see: outside it, `404`. Every `403` and `409` names its reason in a stable code ([Errors](/reference/api/errors/#refusals)). <Status building="api/dashboard-fields" />

## Preconditions: If-Match and ETags

Reading a single resource returns an `ETag`; changing it requires that value in `If-Match`. Each resource names its own version:

| Resource | `ETag` | `If-Match` required by | Stale |
|---|---|---|---|
| An entry's draft | `"<revision id>"` | `PUT` of the entry | `409`, `stale_revision` |
| A content type | `"v<version>"` | `PUT` of the content type | `409`, `stale_version` |
| A role | `"v<version>"` | `PUT` and `DELETE` of the role | `409`, `stale_role` |
| A changeset | `"sha256:<hex>"`, its hash | `submit`, `refresh`, `reviews`, `publish`, `schedule` | `409`, `stale_changeset` |
| A delivery answer | `"<32 hex digits>"` | (read with `If-None-Match`) | `304 Not Modified` when unchanged |

- `If-Match` holds exactly one strong tag, as the `ETag` header gave it. A missing one is `428` `precondition_required`; a malformed one, a weak `W/` tag, a list or `*` is `400` `bad_request`.
- A stale `If-Match` writes nothing, and the `409` carries an `ETag` header naming the current version, so a client can show what changed and try again ([Saves and conflicts](/concepts/saves-and-conflicts/)).

## Idempotency keys

Six requests create or change something that a retry must not do twice, and require an `Idempotency-Key` header:

| Route | Operation |
|---|---|
| `POST {P}/collections/{collection}/entries` | `entry.create` |
| `PUT {P}/collections/{collection}/entries/{entry}` | `entry.save` |
| `POST {P}/content-types` | `content_type.create` |
| `PUT {P}/content-types/{slug}` | `content_type.update` |
| `POST {P}/changesets` | `changeset.create` |
| `POST {P}/changesets/{changeset}/publish` | `changeset.publish` |

- A key is 16 to 128 characters of `A-Z a-z 0-9 - _`. Missing is `428` `precondition_required`; anything else is `400` `bad_request`. Use one key per logical operation, and the same key for every retry of it.
- A key belongs to you, the project, the environment and the operation: the same string used by someone else, elsewhere or for another operation is a different key.
- **Same key, same request**: the recorded answer comes back with its original status, its `ETag`, and `Idempotent-Replayed: true`. "Same request" compares a hash of what the request means: the canonical content or definition, the path's ids, the `If-Match` value and, for a changeset, its title and sorted entry ids. The same content with its keys in another order is the same request.
- **Same key, different request**: `409` `conflict` with `details.reason` `idempotency_key_reused`.
- **Two at once**: the second waits for the first, then replays its answer, or does the work itself if the first rolled back.
- **Only successes are recorded**, in the same transaction as the change. A request that failed, or whose connection dropped before commit, left no record, so retrying it with the same key is safe ([crash tests](/operations/crash-tests/) F01 and F02).
- **How long a key is kept**: today, forever; nothing deletes the records yet. The retention job will keep them at least 7 days ([module 02](/design/modules/02-db/)). <Status planned="P6" />

The other creates take no key. Creating a project or a role with a key that already exists is `409`, so a retry can't make two. Creating an invitation or a website connection twice makes two; the extra one can be revoked or turned off.

## Generations: Colophon-Generation

Each publish takes the environment's next **generation**, a number that only moves forward ([Releases and generations](/concepts/releases-and-generations/)). The `Colophon-Generation` header names one:

- on every `/content/v1` answer, `304` included: the release it was read at;
- on `GET {P}/releases`: the environment's latest generation, `0` before the first publish;
- on a `409` `generation_moved` from publishing: the generation another release moved the environment to;
- on every webhook and purge the worker sends ([Webhooks](/reference/webhooks/)).

## Request ids

Every response has an `x-request-id` header. Send your own (1 to 64 characters of `A-Z a-z 0-9 . _ -`) and it is kept and echoed; anything else is replaced by a fresh UUIDv7. Errors repeat it as `request_id`, so a person can quote one string and the logs find the request. Tests: `a_clients_well_formed_request_id_is_kept_and_echoed` and `hostile_request_ids_are_replaced` in `crates/colophon-api/tests/http.rs`.

## Pagination

Two lists page with a cursor: [an entry list](/reference/api/entries/#get-pcollectionscollectionentries) and a [delivery list](/reference/delivery-api/#get-contentv1projectenvironmentcollection).

- `limit`: 1 to 100, default 20. Outside that is `400` `bad_request`.
- `cursor`: the `next_cursor` of the previous page, opaque. `next_cursor` is `null` on the last page.
- A cursor only works for the list it came from: one from another project, environment or collection is `400` `bad_request`, "this cursor is not valid for this list". A delivery cursor also carries the release its first page was read at.

Every other list answers on one page, with `next_cursor` always `null`, and is capped:

| List | Order | At most |
|---|---|---|
| Projects (`GET /api/v1/projects`, `/me`) | by name | 1,000 |
| Members | owners first, then by name | 1,000 |
| Pending invitations | newest first | 1,000 |
| Roles | built-in first, then as created | 100 (a project's maximum) |
| Content types | by slug | all |
| A version history | newest first | 100 |
| Changesets | newest first | 100 |
| Releases | newest first | 100 |
| Website connections | as created | 100 |
| Jobs | newest release first | 100 |
| Schedules | pending ones by time, then finished ones newest first | 100 pending and 20 finished |

## Limits

| Limit | Default | Setting | Over it |
|---|---|---|---|
| Request body | 1 MiB | `COLOPHON_MAX_BODY_BYTES` (1 KiB to 16 MiB) | `413` `payload_too_large` |
| Producing the response headers | 10 s | `COLOPHON_REQUEST_TIMEOUT_MS` (100 to 120,000) | `504` `deadline_exceeded` |
| Database connections | 16 | `COLOPHON_DB_MAX_CONNECTIONS` (1 to 1,000) | a request that waits 2 s for one gets `503` `unavailable` with `Retry-After: 1`; nothing was attempted |
| One database statement | 5 s | fixed | `500` `internal` |
| A rich-text document | 256 KiB, 16 levels, 10,000 nodes | fixed by the [format](/reference/rich-text/) | `422`, `too_large`, `too_deep` or `too_many_nodes` |

A body whose declared `Content-Length` is over the limit is refused before any handler runs. The server reads and discards what a client sends but a handler didn't read, up to 32 MiB within 2 seconds, so the connection stays usable; past that, the response says `Connection: close` (`crates/colophon-api/src/body.rs`).

The only rate limit is on setup codes ([below](#other-rules)), so `POST /api/v1/setup` is the one route that answers `429`. The worker honours `429` from the sites it calls.

## Status codes

| Code | Meaning |
|---|---|
| `200`, `201` | Done. Creates answer `201`. There is no `Location` header: the body holds the new resource and its id. |
| `204` | Done, nothing to return. |
| `303` | Sign-in redirects (`/auth/login`, `/auth/callback`). |
| `304` | Delivery only: your `If-None-Match` matches. |
| `400` | A malformed request, header, query parameter or path id: `bad_request`, `malformed_json` or `sign_in_failed`. |
| `401` | Not signed in, or the session or token ended. |
| `403` | Signed in, and not allowed this action in a project you can see; or a browser write without its `Origin` and CSRF token. |
| `404` | Missing, or hidden from you: the two look the same. |
| `405` | The path exists, not for this method; the `Allow` header lists the methods. |
| `409` | A conflict: a stale version, another release first, a reused idempotency key, a taken slug, the last owner. |
| `410` | Delivery only: published, then taken down. |
| `413` | The body is over the limit. |
| `415` | Not `Content-Type: application/json`. |
| `422` | Valid JSON that breaks a rule; `details` names the rule and a JSON Pointer to the value. |
| `428` | A required `If-Match` or `Idempotency-Key` is missing. |
| `429` | Too many setup-code attempts; `Retry-After` says how long to wait. |
| `500` | A bug or an unexpected failure; the cause is logged, never shown. |
| `503` | At capacity (with `Retry-After`), or a dependency is missing or down. |
| `504` | The request ran out of time. |

The [errors page](/reference/api/errors/) lists every `code` and every `details.reason`.

## Other rules

- **No CORS.** No route sends `Access-Control-Allow-Origin`. The Studio is served from the API's own origin, so it needs none; other browser pages can't call `/api/v1` with the session cookie. Read `/content/v1` from your site's server or build, or put it behind a proxy on your site's origin.
- **`HEAD`** works wherever `GET` does.
- **Rate limits.** Only setup-code attempts are limited today: five a person and twenty in all per fifteen minutes, counted in PostgreSQL, answered with `429` `rate_limited` and `Retry-After` ([`POST /api/v1/setup`](/reference/api/auth/#post-apiv1setup)). Limits on other routes are planned in milestone S. <Status planned="S" />
- **Unknown query parameters are ignored**; a known one that doesn't parse (`limit=ten`) is `400` `bad_request`.
- **The Studio.** With `COLOPHON_STUDIO_DIR` set, `/studio/` serves the Studio's files, and its `200.html` for any other path under it, with `X-Content-Type-Options: nosniff`, `X-Frame-Options: DENY`, `Content-Security-Policy: frame-ancestors 'none'` and `Referrer-Policy: same-origin` on every response. Any path no route matches answers `404` `route_not_found`.
- **Health.** `GET /livez` checks only that the process answers; `GET /readyz` reports whether it can serve. See [Sign-in and sessions](/reference/api/auth/#get-livez) and [Health checks](/operations/health-checks/).

## Design

[Module 04, API](/design/modules/04-api/); decisions D-034, D-035, D-041 and D-044 to D-048 in the [decisions](/design/decisions/), and D-054, D-055 and D-058 to D-061.
