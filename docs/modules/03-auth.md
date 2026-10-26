# 03 · auth (`crates/colophon-auth`)

> Who is asking, and are they allowed? One policy for people, scripts and agents, with passwords and MFA delegated to
> an identity provider.

| | |
|---|---|
| Milestones | P1 (OIDC, sessions, roles, invitations, cross-project denial), P2 (approval policy), P4 (preview capabilities), P5 (machine credentials, device login, kill switch) |
| Proposed owner | Claude builds; every authentication change gets a manual line-by-line review (spec §34) |
| Depends on | domain (`Actor`, `Scope`, actions), `reqwest` and `jsonwebtoken` (D-031), `aws-lc-rs` (random tokens, SHA-256, constant-time comparison) |
| Used by | api, delivery (preview capabilities), automation, cli (login flows); db implements its session and credential stores |
| Spec | §04, §06, §13, §26, §27 |

## Why it exists

Every request has to be tied to a principal with an explicit scope, and every action has to be checked against the
object, the project, the environment and the field. If authorization were scattered across handlers, one forgotten
check would leak another project's drafts. Inventing a password or cryptography system would be worse, so login and
MFA belong to a proven identity provider, while CMS authority stays in Rust and Postgres (spec §13).

## End goal

Every request carries an authenticated principal with an explicit scope (or is one of the narrow public routes).
Every read and mutation is checked by one deny-by-default policy function, revocation takes effect before the next
operation, and the cross-project and identity test families from spec §27 pass.

## Responsibilities

- OIDC sign-in, sessions and CSRF protection.
- Owner bootstrap and invitations.
- Roles and memberships.
- Machine credentials for scripts and agents.
- The policy function.
- Preview capabilities.
- The row-level-security context for each transaction.

## Not its job

- Storing passwords or running MFA. The identity provider does that.
- Domain rules such as "an agent cannot approve its own proposal". The domain enforces them using the actor that auth
  supplies.
- Hostile-tenant open registration (spec §01 non-goal).

## Breakdown

### Sign-in (spec §13; built in step 1.1, D-031)
- Authorization Code flow with PKCE, `state` and `nonce` checks.
- Exact redirect URIs and exact callback origins.
- **Keycloak** is the reference provider, behind an adapter boundary. Another provider can replace it once it passes the
  same contract tests (spec §04).
- A person's identity is `(issuer, subject)`. An email claim is never used to establish identity or pick an owner.
- `colophon_auth::OidcClient` is the relying party, built on `reqwest` and `jsonwebtoken` rather than an OIDC crate:
  - discovery is lazy and checked: the document's `issuer` must equal the configured issuer;
  - ID tokens need an allowlisted asymmetric algorithm (RS256, PS256, ES256, EdDSA) and a key from the provider's
    JWKS, which is refreshed at most once a minute when an unknown key id appears;
  - `iss` and `aud` must match exactly, `azp` when present, `exp` and `nbf` with a minute of leeway, and the nonce;
  - every provider request has a timeout and a 64 KiB response limit, and redirects are not followed.
- The routes live in `colophon-api` (`/auth/login`, `/auth/callback`, `/auth/logout`, `/me`). A sign-in attempt is a
  row keyed by the hash of its `state`, bound to the starting browser by the `colophon_signin` cookie, valid for 10
  minutes and usable once. `return_to` accepts only a path on this site.

### Owner bootstrap (spec §04)
- A one-time, local `colophon admin bootstrap` command binds the first owner to an authenticated issuer and subject.
- Bootstrap is then permanently disabled.
- A development-only identity exists for local work and refuses to run on a publicly bound install: the Keycloak
  realm in `deploy/keycloak` has a public client secret, and the server refuses to start with it on a non-loopback
  listen address (D-031).

### Sessions (built in step 1.1, D-032)
- Rust issues opaque, random, revocable session tokens and stores only their hash in Postgres.
- Cookies are `Secure`, `HttpOnly`, with `SameSite`; sessions have expiry and idle timeout.
- Cookie-authenticated writes need a CSRF token **and** a matching `Origin`.
- Defaults: a session ends 12 hours after sign-in or after 2 hours unused. Its last use is written at most once a
  minute, decided from the row just read, so most requests only read.
- The CSRF token is per session, stored hashed, and sent to the page in a readable `SameSite=Strict` cookie; writes
  echo it in `X-CSRF-Token`. Trusted origins are the public origin plus `COLOPHON_TRUSTED_ORIGINS`, compared exactly.
- Nothing caches sessions, so revocation (logout) takes effect on the next request.
- CORS is narrowly scoped, and no secrets ever appear in URLs.
- Sensitive actions require fresh authentication: deleting a project, export, credential and role changes.

### Roles and memberships (spec §13)

| Principal | Default authority |
|---|---|
| Owner | Configure the install, identity providers and memberships; sensitive actions need fresh authentication |
| Developer | Manage approved schemas and integrations; production migrations need separate permission |
| Editor | Create and edit allowed collections; request review |
| Publisher | Approve and publish allowed content; does not automatically administer billing or credentials |
| Agent service account | Explicit project/environment/collection/action scopes; draft-only by default, with expiry and quotas |
| Public reader | Published public projections only; no access to drafts or private assets |

### Invitations (spec §04)
- Single-use, expiring after a configurable period (default 72 hours).
- Bound to the authenticated, verified identity that accepts them.
- The invitee sees the project name, role and capabilities before accepting.

### Machine credentials (P5)
- Scoped to project, environment, collections and actions.
- Expiring, with quotas, stored hashed and compared in constant time, as session tokens are.
- The CLI signs in through a browser or device-authorization flow.
- A broad administrator token never appears in a template or frontend bundle.
- A **kill switch** revokes a principal and prevents new work; actions it already completed stay auditable.

### The policy function
- `authorize(principal, action, resource) -> Decision`, deny by default.
- It checks object ownership, project, environment, action and field policy on **every** read and mutation.
- Publish, export, schema change and secret administration need explicit permission.
- Hidden resources answer like missing ones (consistent 404), so IDs can't be probed.
- Revocation is checked before every later authorized operation, including the worker's next external attempt.
- The decision sets the transaction-local row-level-security context that [db](02-db.md) relies on.

### Preview capabilities (P4; spec §06)
- A short-lived capability, 15 minutes by default, scoped to project, environment, the exact revision set and the
  permitted asset IDs.
- It is validated on every protected fetch from the separate preview origin.
- It is never a public cache key.

## Invariants

| ID | Invariant |
|---|---|
| A1 | No request reaches a domain service without a principal, except public delivery, public health probes and the OIDC callback. |
| A2 | Scope comes from the session or credential, never from a user-supplied ID alone. |
| A3 | Identity is `(issuer, subject)`; email never establishes identity or ownership. |
| A4 | Revocation is checked before each later operation by the revoked principal. |
| A5 | Agents are draft-only unless a scoped, revocable, opt-in permission says otherwise; nobody approves their own permission escalation. |
| A6 | Hidden and missing resources are indistinguishable to unauthorized callers. |
| A7 | Bootstrap runs once and is then disabled; the development identity never runs on a public bind. |
| A8 | Tokens and session secrets are stored hashed, compared in constant time, and never logged. |

## Interfaces

- **Axum extractors:** `SignedIn` (step 1.1, in `colophon-api`: the identity and session, plus CSRF and `Origin`
  checks for writes); `Principal` and `RequireFreshAuth` replace it as roles and machine credentials arrive.
- **Policy:** `authorize(principal, action, resource)`.
- **Session store and credential store** ports, implemented in [db](02-db.md).
- **Identity-provider adapter:** discovery, code exchange, token validation.
- **Preview:** capability issue and verify.

## Failure modes

| Situation | Behaviour |
|---|---|
| Identity provider down | Existing valid sessions continue per policy; new sign-ins fail with a clear message (X11) |
| Cache down | No effect: sessions and memberships live in Postgres |
| Expired or revoked session | 401, with a sign-in path; no partial action |
| Wrong project or environment | 404 (hidden) or 403 per the visibility policy; never another project's data |
| Suspected credential leak | Revoke the principal, rotate related secrets, keep the audit trail; revocation does not undo completed external effects (spec §33) |

## Tests and evidence

- **Spec §27 identity and policy cases:**
  - revoked session and expired token;
  - wrong project or environment;
  - hidden field;
  - unauthorized publish and export;
  - stale approval.
- **Browser safety:**
  - CSRF without a token, and a wrong `Origin`;
  - unauthorized cross-origin calls;
  - unsafe redirects.
- **Fixtures:** two projects with colliding slugs and entry IDs, across the API, search, export, media, cache and worker
  paths (spec §13).
- **Invitations:** expired, reused, or accepted by a different identity.
- **Bootstrap:** fails once it is complete; the development identity refuses a public bind.
- **Contract tests** for the identity-provider adapter against Keycloak.
- **In place since step 1.1:**
  - `crates/colophon-auth/tests/id_token.rs`: every forged or mismatched ID token is refused, with keys generated per
    run (another key under the provider's key id, nonce, issuer, audience, expiry, `nbf`, subject, `azp`, key id,
    HS256 signed with the public key, `alg: none`);
  - `crates/colophon-server/tests/sign_in.rs`, against the real binary and Keycloak's login form: sign-in, `/me`,
    writes refused without CSRF or a trusted `Origin`, logout and 401 afterwards; a project invisible to non-members;
    a callback that works once and only in the browser that started it;
  - the server's configuration tests: the development client secret is refused off loopback;
  - `apps/testbed/tests/session.spec.ts`: two browsers; a project created by one is invisible to the other;
  - `perf/k6/session.js`: every request authenticated by cookie, at least 1,000,000 requests per full run.

## Build order

1. **P1:** OIDC with Keycloak, sessions, CSRF/Origin and cross-project denial for projects (step 1.1, done); bootstrap,
   roles, invitations and the policy function.
2. **P2:** approval permissions and self-review recording.
3. **P4:** preview capabilities.
4. **P5:** machine credentials, device-authorization login, quotas, kill switch.

## Open decisions

- Whether a hidden resource answers 404 everywhere, or 403 for members of the same project.
