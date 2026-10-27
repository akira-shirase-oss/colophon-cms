<script>
	import Status from '#lib/components/Status.svelte';
</script>

How a person or a program proves who they are: the health checks that need no one, OpenID Connect sign-in for browsers, sessions, `/me`, and the device sign-in the command line uses to get a personal bearer token. The rules every request shares (cookies, CSRF, `Origin`, bearer tokens) are on [API conventions](/reference/api/#authentication); the security reasoning is in [Roles and permissions](/concepts/roles-and-permissions/) and the [Security model](/resources/security/). Handlers: `crates/colophon-api/src/health.rs`, `auth.rs`, `session.rs` and `device.rs`.

## Health

Both checks answer without signing in, with `Cache-Control: no-store`, and never touch a dependency while answering: background probes keep the state they report. What to point a load balancer or an orchestrator at is on [Health checks](/operations/health-checks/).

### `GET /livez`

Whether the process answers. It checks nothing else, because restarting a process can't fix a database outage.

```json
{ "status": "live" }
```

Always `200`.

### `GET /readyz`

Whether this process can serve now, from the last background probe of each dependency.

```json
{ "status": "degraded", "role": "all", "checks": { "database": "up", "cache": "down" } }
```

| `status` | When | HTTP |
|---|---|---|
| `ready` | The database is up, and the cache is up or not configured. | `200` |
| `degraded` | The database is up, the cache down or not probed yet: reads are slower, never unsafe. | `200` |
| `starting` | The database hasn't been probed yet. | `503` |
| `not_ready` | The database is down. | `503` |
| `draining` | Shutting down, whatever the probes say. | `503` |

`role` is what this process runs: `api`, `worker` or `all`. Each check is `up`, `down`, `unknown` (not probed yet) or `not_configured`. The `503` body is this report, not the error envelope. The body never names hosts, ports or error messages.

## Signing in from a browser

A browser signs in through the identity provider configured with `COLOPHON_OIDC_ISSUER`, `COLOPHON_OIDC_CLIENT_ID` and `COLOPHON_OIDC_CLIENT_SECRET` (Keycloak locally; [Configuration](/operations/configuration/)). Colophon verifies the ID token itself. Sessions are rows in PostgreSQL; the cookie holds a random token, and only its SHA-256 is stored.

### `GET /auth/login`

Starts a sign-in and sends the browser to the provider, with a fresh `state`, `nonce` and PKCE verifier.

- **Who:** anyone. A browser follows it; there is nothing to call it with from a script.
- **Query:** `return_to`, where to go afterwards: a path on this server, at most 2,048 characters, starting with one `/`, without `\` or control characters. Anything else, including an absolute URL or `//host`, becomes `/`.

**Response:** `303 See Other` to the provider's authorization URL, with `Cache-Control: no-store` and a cookie that binds the attempt to this browser:

```http
HTTP/1.1 303 See Other
Location: https://id.example.com/realms/colophon/protocol/openid-connect/auth?response_type=code&client_id=colophon-server&state=…&nonce=…&code_challenge=…
Set-Cookie: colophon_signin=…; Path=/auth; Max-Age=600; Secure; SameSite=Lax; HttpOnly
Cache-Control: no-store
```

The attempt works once, within 10 minutes.

| Status | `code` | When |
|---|---|---|
| `503` | `unavailable` | Sign-in isn't configured on this server, the provider is unreachable, or the server has no database. |

### `GET /auth/callback`

Where the provider sends the browser back. It finishes the sign-in, creates the session and returns to `return_to`.

- **Who:** the browser that started the sign-in: the `state` must match its `colophon_signin` cookie.
- **Query:** `code` and `state` from the provider, or `error` when the provider refused.

**Response:** `303 See Other` to `return_to`, `Cache-Control: no-store`, and three cookies:

```http
HTTP/1.1 303 See Other
Location: /studio/
Set-Cookie: colophon_session=…; Path=/; Max-Age=43200; Secure; SameSite=Lax; HttpOnly
Set-Cookie: colophon_csrf=…; Path=/; Max-Age=43200; Secure; SameSite=Strict
Set-Cookie: colophon_signin=; Path=/auth; Max-Age=0; Secure; SameSite=Lax; HttpOnly
```

`Max-Age` is the session's lifetime, 12 hours; a session also ends after 2 hours unused. The person's identity is their issuer and `subject` at the provider; their display name comes from the ID token.

| Status | `code` | When |
|---|---|---|
| `400` | `sign_in_failed` | The provider returned an error, `code` or `state` is missing, the `state` isn't this browser's, the attempt is unknown, expired or already used, or the ID token failed verification. The cause is logged, never shown: it could help an attacker. |
| `503` | `unavailable` | Sign-in isn't configured, or the server has no database. |

### `POST /auth/logout`

Ends the session that sent it, or revokes the bearer token that sent it.

- **Who:** signed in. With a cookie, it is a write: it needs the `Origin` and `X-CSRF-Token` headers.
- **Body:** none.

**Response:** `204 No Content`. For a session, it also clears both cookies (`Max-Age=0`); the session is revoked in the database, so the old cookie stops working on the next request. For a token, it is revoked; `colophon logout` calls this.

| Status | `code` | When |
|---|---|---|
| `401` | `unauthorized` | No session, or it already ended. |
| `403` | `forbidden` | A cookie write without a trusted `Origin` or a matching CSRF token. |

### `GET /me`

Who is signed in, whether they are an administrator, and their projects.

- **Who:** signed in, with a session or a token.

**Response:** `200`.

```json
{
  "data": {
    "id": "0193f69e-0a1b-7c2d-8e3f-405162738495",
    "issuer": "http://localhost:58080/realms/colophon",
    "subject": "f3c1a9e2-6b7d-4c8e-9a0b-1c2d3e4f5a6b",
    "display_name": "Ada Owner",
    "administrator": true,
    "projects": [
      {
        "id": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f",
        "slug": "field-notes",
        "name": "Field Notes",
        "role": "owner",
        "role_name": "Owner"
      }
    ]
  }
}
```

| Field | Type | Meaning |
|---|---|---|
| `id` | UUID | Your identity in Colophon. |
| `issuer`, `subject` | string | Who you are at the identity provider; the operator needs `subject` for `colophon admin bootstrap`. |
| `display_name` | string or null | From the provider. |
| `administrator` | boolean | Whether you may create projects (D-043). |
| `projects` | array | Your projects, by name, at most 1,000: `id`, `slug`, `name`, your `role` (its key) and `role_name` <Status building="R.2" />. |

| Status | `code` | When |
|---|---|---|
| `401` | `unauthorized` | Not signed in, or the session or token ended. |

## Signing in from a terminal

Device sign-in has the shape of the OAuth device authorization grant (RFC 8628), approved through Colophon's own browser session:

1. The client calls `POST /auth/device` and gets a secret to poll with, a short code, and a link.
2. The person opens the link in a browser where they are signed in; the Studio shows what is asking (`GET /auth/device/{code}`) and approves it (`POST /auth/device/approve`).
3. The client polls `POST /auth/device/token` with the secret: `pending`, then a personal access token, exactly once.

Codes live 10 minutes; tokens 30 days. Only hashes of the secret and the token are stored. Only a browser session may approve, so a token can't mint more tokens. The command line does all of this in `colophon login` ([Command line](/reference/cli/)).

### `POST /auth/device`

Starts a device sign-in.

- **Who:** anyone; no sign-in.
- **Body:**

```json
{ "client_name": "colophon-cli on build-01" }
```

| Field | Type | Rules |
|---|---|---|
| `client_name` | string | Required. What the approval page shows. Trimmed, 1 to 100 characters, no control characters. |

**Response:** `201 Created`.

```json
{
  "data": {
    "device_secret": "x2V9q0mJ8bS6Zr1T4kWn7YcPd3Lf5Ga0Hu2Ei9Oj6Qs",
    "user_code": "KMTX-48ZR",
    "verification_uri": "http://localhost:58000/studio/device?code=KMTX-48ZR",
    "interval": 2,
    "expires_in": 600
  }
}
```

| Field | Meaning |
|---|---|
| `device_secret` | 43 characters. Keep it to poll with; never show it. |
| `user_code` | Two groups of four from `BCDFGHJKLMNPQRSTVWXZ23456789`: no vowels, nothing that looks alike. |
| `verification_uri` | The Studio's approval page on the server's public origin. |
| `interval` | Seconds to wait between polls. |
| `expires_in` | Seconds the code works. |

| Status | `code` | `details.reason` | When |
|---|---|---|---|
| `422` | `validation_failed` | `invalid_client_name` | `client_name` is blank, too long or has control characters. |
| `503` | `unavailable` | | Three codes in a row clashed with live ones (`Retry-After: 1`). |

### `GET /auth/device/{code}`

What is asking to sign in, for the approval page.

- **Who:** signed in (a session or a token). Only someone signed in may see what is asking.
- **Path:** `code`, the user code, in any letter case.

**Response:** `200`.

```json
{ "data": { "client_name": "colophon-cli on build-01", "expires_at": "2026-10-27T09:24:03.512841Z", "approved": false } }
```

| Status | `code` | When |
|---|---|---|
| `401` | `unauthorized` | Not signed in. |
| `404` | `not_found` | No live sign-in has this code: unknown, or expired. |

### `POST /auth/device/approve`

Lets the device sign in as you.

- **Who:** signed in with a browser session, which means the `Origin` and `X-CSRF-Token` headers too. A bearer token is refused.
- **Body:**

```json
{ "user_code": "KMTX-48ZR" }
```

| Field | Type | Rules |
|---|---|---|
| `user_code` | string | Required, any letter case. |

**Response:** `204 No Content`.

| Status | `code` | When |
|---|---|---|
| `401` | `unauthorized` | Not signed in. |
| `403` | `forbidden` | Signed in with a bearer token ("approve a device from a browser where you are signed in"), or a cookie write without its `Origin` and CSRF token. |
| `404` | `not_found` | No live, unapproved sign-in has this code. |

### `POST /auth/device/token`

Polls for the token.

- **Who:** the device that holds the secret; no sign-in.
- **Body:**

```json
{ "device_secret": "x2V9q0mJ8bS6Zr1T4kWn7YcPd3Lf5Ga0Hu2Ei9Oj6Qs" }
```

**Response:** `200`, either still waiting:

```json
{ "data": { "status": "pending" } }
```

or approved, exactly once (the sign-in is deleted as it is read):

```json
{ "data": { "status": "approved", "access_token": "Vq4…43 characters…", "expires_at": "2026-11-26T09:15:40.118204Z" } }
```

Send the token as `Authorization: Bearer` followed by it. It lasts 30 days; its last use is recorded at most once a minute.

| Status | `code` | When |
|---|---|---|
| `404` | `not_found` | Unknown, expired or already exchanged: start again. A secret that isn't 43 characters is also `404`. |

## Not built yet

- API tokens for scripts and agents, with the same permissions as roles, read-only or full access. <Status planned="R.4" />
- Asking for a fresh sign-in before sensitive changes such as role changes. <Status planned="not scheduled" />
- More than one administrator, through a grant flow. <Status planned="not scheduled" />
