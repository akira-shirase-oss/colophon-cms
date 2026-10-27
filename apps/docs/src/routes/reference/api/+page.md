The management API lives under `/api/v1` on the Colophon server, beside sign-in (`/auth/*`), `/me` and the health checks. The Studio, the command line and scripts all use it, with the same checks: there is no private path.

## Addresses

Most routes sit under a project and an environment:

```text
/api/v1/projects/{project}/environments/{environment}/…
```

`{project}` is the project's id (a UUID). `{environment}` is the environment's slug; every project starts with `production`, and creating others isn't built yet. A project you aren't a member of answers `404`, exactly as one that doesn't exist.

## Authentication

| Client | How it signs in | What it sends |
|---|---|---|
| A browser (the Studio) | `GET /auth/login`, OpenID Connect through the configured provider | The `colophon_session` cookie (`HttpOnly`, `Secure`, `SameSite=Lax`). Writes also need an `Origin` equal to the server's public origin (or one in `COLOPHON_TRUSTED_ORIGINS`) and an `X-CSRF-Token` header equal to the readable `colophon_csrf` cookie. |
| The command line, scripts | Device sign-in: `POST /auth/device`, approved by a signed-in person at `/studio/device` | `Authorization: Bearer <token>`. No CSRF token. An invalid bearer token never falls back to the cookie. |

`GET /me` returns who you are, your projects, your `subject` at the identity provider and whether you are an `administrator`. `POST /auth/logout` ends the session, or revokes the bearer token that sent it.

## Responses

- One resource: `{"data": …}`. A list: `{"data": [...], "next_cursor": "…" | null}`.
- Every response has an `x-request-id` header (a UUIDv7, or your own if it is at most 64 characters of `A-Z a-z 0-9 . _ -`). Errors repeat it in the body, so a person can quote one string and the logs find the request.
- Errors are always the [error envelope](/reference/api/errors/), including errors from the middleware: `{"error": {"code", "message", "details"?, "request_id"}}`.

## Preconditions: If-Match and ETags

Reading a single resource returns an `ETag`; changing it requires that value in `If-Match`. Each resource names its own version:

| Resource | ETag | Stale |
|---|---|---|
| An entry's draft | `"<revision id>"` | `409`, `stale_revision` |
| A content type | `"v<version>"` | `409`, `stale_version` |
| A changeset | `"sha256:<hex>"`, its hash | `409`, `stale_changeset` |

A missing `If-Match` is `428`; a malformed one, or `If-Match: *`, is `400`. A stale one writes nothing, and the response's `ETag` names the current version.

## Idempotency keys

Creates, saves and publishes require an `Idempotency-Key` header: 16 to 128 characters of `A-Z a-z 0-9 - _`, one per logical operation, reused on every retry of it.

- Same key, same request: the recorded answer comes back, with `Idempotent-Replayed: true`.
- Same key, different request: `409`, `details.reason` `idempotency_key_reused`.
- Only successful answers are recorded, so a failed attempt can simply be retried.

Creating an invitation takes no key: a retry makes a second link, and the first can be revoked.

## Pagination

Lists take `limit` (default 20, 1 to 100) and `cursor`, and return `next_cursor` until the end. A cursor only works for the list it came from: one from another project, environment or collection is refused. Entry lists also take `q`, which searches titles. Some short lists (changesets, members, invitations) return their newest 100 or 1,000 without a cursor.

## Limits

| Limit | Default | Setting |
|---|---|---|
| Request body | 1 MiB (`413` over it) | `COLOPHON_MAX_BODY_BYTES` |
| Request deadline | 10 s (`504`, `deadline_exceeded`) | `COLOPHON_REQUEST_TIMEOUT_MS` |
| Database pool | 16 connections; a request that can't get one within 2 s gets `503` with `Retry-After: 1`, and nothing was attempted | `COLOPHON_DB_MAX_CONNECTIONS` |
| Rich-text document | 256 KiB, depth 16, 10,000 nodes | fixed by the [format](/reference/rich-text/) |

## Status codes

| Code | Meaning |
|---|---|
| 200, 201 | Done. Creates answer `201`. |
| 304 | Delivery only: your `If-None-Match` matches. |
| 400 | A malformed request or header. |
| 401 | Not signed in, or the session ended. |
| 403 | Signed in, and not allowed this action on a resource you can see. |
| 404 | Missing, or hidden from you: the two look the same. |
| 409 | A conflict: a stale version, another release first, a reused idempotency key, a taken slug, the last owner. |
| 410 | Delivery only: published, then taken down. |
| 413 | The body is over the limit. |
| 415 | Not `Content-Type: application/json`. |
| 422 | Valid JSON that breaks a rule; `details` names the rule and a JSON Pointer to the value. |
| 428 | A required `If-Match` or `Idempotency-Key` is missing. |
| 503 | A dependency or capacity is unavailable; retry after `Retry-After`. |
| 504 | The request ran out of time. |

There is no per-person rate limit yet, so no `429` from this API; the worker does honour `429` from the sites it calls.

## Health

- `GET /livez`: the process responds. It checks nothing else.
- `GET /readyz`: whether this process can serve, from background probes, so a flood of checks costs nothing. `ready` (200), `degraded` (200: the cache is down, which slows reads but is never unsafe), `starting`, `not_ready` or `draining` (503).
