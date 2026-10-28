<script>
	import Status from '#lib/components/Status.svelte';
</script>

What Colophon does to keep projects apart, people signed in and outbound requests contained, with the file behind each claim, and what it doesn't do yet. Colophon is pre-release: v0.1 is not out, nothing is for production use, and no one outside the project has audited it. This page describes `main` unless a label says otherwise.

## Projects can't see each other

Every table that belongs to a project carries its id, and PostgreSQL's **row-level security** decides what a transaction can see, below the application:

- Each request's transaction says who is asking and in which project with transaction-local settings, `app.identity_id` and `app.project_id` (`set_config(…, true)` in `crates/colophon-db/src/scope.rs`). They end with the transaction, so a pooled connection can't carry them into the next request.
- Project tables enable **and force** row-level security, so the policies apply to the table owner's own queries too. A project is visible only inside its own context or to its members.
- References between tables are composite foreign keys over the project and environment, so a row can never point into another project.
- A project you aren't a member of answers `404`, exactly like one that doesn't exist, so ids can't be probed.

`crates/colophon-db/tests/isolation.rs` runs as the application's real login against a fresh database: the login can't bypass row-level security, a project is invisible to everyone but its members, the context ends with its transaction on a reused connection, and nobody can write into a project from another one's context. The server's tests deny a second project by id on every route.

## Database roles

Each part of the server runs with only the privileges it needs (`deploy/postgres/roles.sql`, and the grants in each migration):

| Role | Logs in | What it may do |
|---|---|---|
| `colophon` | yes | The local stack's superuser: owns the tables and runs migrations. In production, a dedicated owner role that isn't a superuser. |
| `colophon_api` | no | The application's privileges. `NOSUPERUSER NOBYPASSRLS`, owns nothing, and every project table's policies apply to it. |
| `colophon_app` | yes | The login the server uses: a member of `colophon_api` and nothing more. |
| `colophon_delivery` | no | What public delivery may read: published copies, releases and environments' generations. Never drafts. |
| `colophon_worker` | no | What the worker may touch: delivery jobs in every project, their destinations, the releases being announced and which entries a release changed, never their content. |

`colophon_app` holds `colophon_delivery` and `colophon_worker` without inheriting them (`WITH INHERIT FALSE, SET TRUE`): a delivery transaction runs `SET LOCAL ROLE colophon_delivery`, which drops every other privilege until it ends, so a bug in delivery can't read a draft. The worker switches the same way.

Some guarantees are grants, not code:

- `colophon_api` may only `SELECT` and `INSERT` revisions (migration 0002): no code path can change or delete one.
- `colophon_api` reads `administrators` and `installation` and can write neither (migration 0006).
- `delivery_environment`, which resolves a public request's generation, is a `SECURITY DEFINER` function owned by the delivery role with a fixed `search_path`, executable only by `colophon_api` (migration 0011).

The local stack's passwords (`colophon-dev`) are public and only for development.

## Signing in

People sign in through **OpenID Connect**, with Keycloak as the local reference provider. Colophon's relying party is its own code on `reqwest` and `jsonwebtoken` (`crates/colophon-auth/src/oidc.rs`, decision D-031):

- the Authorization Code flow with PKCE, `state` and `nonce`; discovery must report exactly the configured issuer;
- ID tokens signed with an allowlisted asymmetric algorithm only (`RS256`, `PS256`, `ES256`, `EdDSA`), with a key from the provider's JWKS, refreshed at most once a minute;
- `iss` and `aud` exactly, `azp` when present, `exp` and `nbf` with a minute of leeway, and the nonce;
- every request to the provider has a 5-second connect and 10-second total timeout and a 64 KiB response limit, and follows no redirects;
- a sign-in attempt is a row keyed by the SHA-256 of its `state`, bound to the browser that started it by the `colophon_signin` cookie, valid for 10 minutes and usable once; `return_to` accepts only a path on this site.

A person is `(issuer, subject)`. An email address never establishes identity or ownership. The development realm's client secret is public, so the server refuses to start with it on a listen address that isn't loopback.

`crates/colophon-auth/tests/id_token.rs` refuses every forged or mismatched ID token, with keys generated per run: another key under the provider's key id, a wrong or missing nonce, wrong issuer or audience, expired, not yet valid, no subject, a foreign `azp`, a missing or unknown key id, `HS256` signed with the public key, and `alg: none`. `crates/colophon-server/tests/sign_in.rs` signs in through the real Keycloak.

## Sessions and CSRF

- **Sessions are rows in PostgreSQL.** The `colophon_session` cookie (`HttpOnly; Secure; SameSite=Lax`) holds 256 random bits; only their SHA-256 is stored, so a database leak yields no usable session (D-032).
- A session ends **12 hours after sign-in or after 2 hours unused**. Its last use is written at most once a minute.
- Nothing caches sessions: revoking one (signing out) takes effect on the very next request.
- **Every cookie-authenticated write** (anything but `GET`, `HEAD` and `OPTIONS`) needs an `Origin` that exactly matches the public origin or one in `COLOPHON_TRUSTED_ORIGINS`, **and** an `X-CSRF-Token` equal to the session's CSRF token. That token is stored hashed and reaches the page in a readable `colophon_csrf` cookie marked `SameSite=Strict`. Secrets are compared in constant time.
- The Studio is served by the same server at `/studio`, so none of this needs CORS, and the server sends no CORS headers. Its responses say `nosniff`, `X-Frame-Options: DENY`, `frame-ancestors 'none'` and `Referrer-Policy: same-origin`.

## Device sign-in and bearer tokens

The command line can't hold a browser session, so it signs in with a device code that a signed-in person approves in the Studio (D-041, `crates/colophon-api/src/device.rs`):

- codes live 10 minutes and tokens 30 days; both are stored as SHA-256 hashes;
- only a browser session can approve a code, so a token can't mint more tokens;
- `Authorization: Bearer` is accepted on every route and needs no CSRF token, because a browser never sends one on its own; an invalid token is `401`, never a fall back to the cookie;
- `colophon logout` revokes the token; revoked tokens are kept, not deleted.

A token acts with its person's full membership rights. Scoped tokens are planned (below). `crates/colophon-db/tests/access.rs` checks that a code is approved once and exchanged once, that expired codes and tokens are dead, and that only hashes are stored.

## Deny by default

Every handler asks one evaluator whether the caller's role allows the action, in the collection when the action is about one, and anything not allowed is refused. Roles are data (R.1), owners can define more, with nobody able to hand out power they don't hold (R.2), and a permission can be narrowed to some fields or to the member's own entries (R.3): a read leaves out the fields a role can't read, a save can't change a field it can't change, and an entry it can't read at all answers `404`, like one that doesn't exist. [Roles and permissions](/concepts/roles-and-permissions/) has the actions, the built-in roles and the rules. <Status done="R.3" />

Signing in grants nothing by itself: people reach a project only by invitation.

## Invitations

An invitation link, `/studio/invite#<token>`, carries a single-use token of 256 random bits that expires after 72 hours. The token travels after the `#`, which browsers never send to a server, so it can't reach access logs, proxies or a `Referer` header; only its SHA-256 is stored, and lookup and acceptance take it in a JSON body, never in a URL. Accepting binds the membership to whoever is signed in. Because email never establishes identity, an invitation isn't tied to an address: whoever holds the link can use it, so send it privately; it can be revoked at any time.

## The first administrator

Only administrators create projects. The first one is made once. The operator can make it from a terminal, with the database owner's credentials: `colophon admin bootstrap --subject S` (D-043). In one transaction it finds or creates the identity, claims the single `installation` row and records the administrator. Run again for the same person it changes nothing (exit 0); for anyone else it is refused (exit 4).

- Once an administrator exists, no HTTP route can create another (before that, only the setup code below can, D-061).
- The `installation` row can't be updated, deleted or truncated, even by the table owner, without dropping its triggers.
- A restrictive row-level security policy, `projects_created_by_administrators`, refuses anyone else's project below the API.

`crates/colophon-db/tests/administrators.rs` and `crates/colophon-server/tests/bootstrap.rs` test each of these, including two bootstraps at the same moment making one administrator.

Step 3.7 added a second way that keeps the rule that only the operator can start it (D-061): while nobody is an administrator, the server prints a one-time setup code to its standard error when it starts, never to the structured log, and a signed-in person who sends it to `POST /api/v1/setup` becomes the first administrator. The code is 100 random bits, used once, expires after an hour, is stored only as a SHA-256 hash, and is compared in constant time with every live code; wrong, used and expired codes get one answer. Attempts are rate-limited in PostgreSQL, so every server instance shares the count: five a person and twenty in all per fifteen minutes. The application role still can't read or write `installation`, `administrators` or the code tables except through four functions that check all of this and refuse once anyone is an administrator. A code in standard error can sit in collected container logs for its hour; it dies at first use. An operator who wants no code ever issued runs the bootstrap command before the first start. `crates/colophon-db/tests/setup.rs` and `crates/colophon-server/tests/setup.rs` test each property ([Onboarding and plain words](/concepts/onboarding/)). <Status done="3.7" />

## Outbound requests only to public addresses

Destination addresses (webhooks, CDNs, probes) come from project members, so without rules a webhook could reach the cloud's metadata endpoint, the database or an admin port (SSRF). Two layers stop it:

- **When saved and before every attempt**, `colophon_domain::destination::check_url` requires `https`, no credentials in the URL, and, for an IP address written in the URL, a public one.
- **At connection time**, the worker's resolver (`crates/colophon-worker/src/guard.rs`) resolves names itself and hands the HTTP client only public addresses. A name that resolves only to private ones fails for good, so DNS can't point a destination inside later.

Not public, by `colophon_domain::destination::is_public`: loopback, private (10/8, 172.16/12, 192.168/16), link-local (including 169.254.169.254), shared (100.64/10), `0.0.0.0/8`, 192.0.0/24, benchmarking (198.18/15), documentation, multicast, broadcast, 240/4, and for IPv6 loopback, unspecified, multicast, unique local (`fc00::/7`), link-local (`fe80::/10`), documentation (`2001:db8::/32`) and NAT64 (`64:ff9b::/96`); IPv4-mapped IPv6 addresses are judged as IPv4. The worker follows no redirects and has a 5-second connect timeout. Only a development server listening on a loopback address may use `http` and private addresses.

## Limits on every request

| Limit | Default | Where |
|---|---|---|
| Request body | 1 MiB (`COLOPHON_MAX_BODY_BYTES`, 1 KiB to 16 MiB) | `crates/colophon-api/src/middleware.rs` |
| Time to produce response headers | 10 s, then `504` (`COLOPHON_REQUEST_TIMEOUT_MS`) | same |
| Waiting for a database connection | 2 s, then `503` with `Retry-After: 1` | `crates/colophon-db/src/lib.rs` |
| Any one SQL statement | 5 s (`statement_timeout`) | same |
| Database pool | 16 connections (`COLOPHON_DB_MAX_CONNECTIONS`); the worker has its own | configuration |
| Rich text | depth 16, 256 KiB of canonical JSON | `crates/colophon-domain/src/rich_text/mod.rs` |
| Pages of entries and public lists | at most 100 | `colophon-db` |
| A publish request | 1 to 100 entries | `colophon-db` |
| Members and invitations listed | 1,000 | `colophon-db` |
| Roles in a project, permissions in a role | 100 roles, 500 permissions | `colophon_domain::policy` |
| Schedules listed | the 100 next pending and the 20 latest finished | `colophon_db::schedules` |
| Setup-code attempts | 5 a person and 20 in all per 15 minutes, then `429` with `Retry-After` | migration 0020 |

The [configuration reference](/operations/configuration/) lists every setting.

## Errors that never leak internals

Every error, including those from middleware and panics, leaves as one JSON envelope: a stable `code`, a `message` and the `request_id` that is also in the `x-request-id` header. An unexpected failure is `500` with code `internal` and the message "internal error"; its cause goes only to the server's log, tagged with the request id. Sign-in failures say only that sign-in failed or expired; the details are logged, never shown, because they can help an attacker. Refusals carry fixed messages written to be safe to show. A client-supplied request id that isn't safe is dropped before anything logs it, and logged paths leave out the query string, which can carry tokens. Configuration secrets (the database URL, the OIDC client secret) are redacted in debug output.

## What isn't done

- **Rate limiting.** There is no general per-client or per-person rate limit; the only one is on setup-code attempts (above). Starting a device sign-in needs no authentication; its table is bounded by clearing expired rows on each insert. Rate limits per person and per address, shared across server instances, are planned in milestone S <Status planned="S" />, and P5's gate includes a noisy agent being throttled (L08). <Status planned="P5" />
- **Audit log and scoped API tokens, not merged.** Step R.4, built on a branch, records every change to roles, memberships, invitations and tokens in an append-only log that even the table's owner can't change, and adds project API tokens (read-only, full access or custom) that act as their own identity, can never manage access or read the log, and stop the moment they are revoked ([API tokens and the audit log](/reference/api/tokens/)). Until it is merged, a bearer token acts with its person's full rights. Budgeted machine credentials for agents come in P5. <Status building="R.4" />
- **Fresh sign-in before sensitive changes**, such as role changes or deleting a project. Module 03 requires it; nothing asks yet. <Status planned="not scheduled" />
- **Signed webhooks.** Announcements carry an idempotency key and the generation but no HMAC signature; that comes with destination secrets. <Status planned="not scheduled" />
- **Secrets management.** Secrets come from environment variables only: no secret store, rotation or encryption at rest, because no third-party credential is stored yet. A real CDN adapter will read its credentials from the environment, never the database.
- **A restrictive Content Security Policy** for the Studio. Its policy today only forbids framing.
- **Private delivery.** Every published project is public; there are no private or preview projections yet. <Status planned="P4" />
- **Several administrators**, through a grant flow. <Status planned="not scheduled" />
- **Sign-up in the development realm.** The local Keycloak realm lets anyone who can reach it create an account; an account grants nothing until an invitation or the setup code, and a production identity provider decides for itself whether to offer sign-up (D-061).
- **A security policy and advisories.** `SECURITY.md` with private advisories through GitHub, and a table of supported versions, are planned for v0.1 ([Releases](/design/releases/)), with signed release artifacts and an SBOM in P6. <Status planned="P6" />
- **An outside review.** No external audit or penetration test has been done. The spec's target is the relevant OWASP ASVS 5.0 Level 2 controls before a public production release, mapped with evidence; that mapping doesn't exist yet. Hostile multi-tenant hosting is not a goal: one installation serves one organisation, with its projects isolated.
- **Backups and restore.** <Status planned="P6" />

## Reporting a problem

There is no private reporting channel yet; it arrives with `SECURITY.md` at v0.1. Until then, Colophon is pre-release and shouldn't hold anything you need to protect.

## Design

[Module 03, auth](/design/modules/03-auth/), [module 02, db](/design/modules/02-db/) and [module 05, worker](/design/modules/05-worker/); decisions D-031, D-032, D-041, D-042, D-043 and D-046 in the [decisions](/design/decisions/). The spec's security requirements are §13, §26 and §27 of the master specification.
