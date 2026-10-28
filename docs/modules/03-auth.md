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
- Built in step 1.8 (D-043):
  - the first owner is the installation's first **administrator**: the only kind of person who can create projects.
    Signing in grants nothing by itself; people reach a project by invitation;
  - `colophon admin bootstrap --subject S [--issuer I]` runs as the migrator, finds or creates the identity, claims
    the single `installation` row and records the administrator, in one transaction. Again for the same identity it
    changes nothing; for anyone else it exits 4. Migration 0006 makes the installation row permanent (no update,
    delete or truncate, even by the table owner) and gives the application role read-only access to both tables;
  - `POST /api/v1/projects` checks `colophon_db::administrators::standing` (403 for a non-administrator, with a
    different message before any bootstrap), and the restrictive policy `projects_created_by_administrators` refuses
    anyone else's project below the API;
  - `/me` shows the subject (what to bootstrap with) and whether you're an administrator; `colophon whoami` prints
    both;
  - more administrators need a grant flow, planned with project administration.
- Since step 3.7 (D-061, amending D-043), a fresh installation can also be set up from the Studio:
  - while nobody is an administrator, a server that serves the API issues a **setup code** at start-up
    (`colophon-server`'s `setup_code` module): 100 random bits as `XXXXX-XXXXX-XXXXX-XXXXX` in Crockford's
    base-32 (`colophon_auth::tokens::setup_code`), valid for an hour, stored as its SHA-256 hash only, and printed
    once to standard error, never to the structured log. The log says only that a code was printed;
  - `POST /api/v1/setup` with `{"code"}` from a signed-in caller (CSRF and `Origin` checked for cookies, like every
    write): the code is hashed on arrival (case, spaces, dashes and the look-alikes O, I and L don't matter), the
    attempt is counted in Postgres (five a person, twenty in all, per fifteen minutes; 429 `rate_limited` with
    `Retry-After`), the hash is compared with every live one in constant time, and the matching code is claimed in
    one transaction that writes the `installation` row and the administrator, exactly as the bootstrap does.
    Wrong, used and expired codes get one answer (422 `setup_code_invalid`); once anyone is an administrator,
    409 `already_set_up`, and later starts issue nothing;
  - the application role reaches the codes only through migration 0020's functions; it still can't read or write
    `installation` or `administrators`. Running `colophon admin bootstrap` before the first start means no code is
    ever issued;
  - the Studio's home page shows a signed-in person on such an installation where to find the code, a field for
    it, and the bootstrap command with their own subject.
- Sign-up belongs to the identity provider: `GET /auth/login?start=create` asks it for its sign-up form
  (`prompt=create`, when its discovery document lists it; its sign-in page otherwise). The development realm allows
  self-registration since step 3.7. A new account grants nothing: it has no projects until someone invites it, and
  creates none unless it is an administrator.
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

Built in step 1.7 (D-042), for people:

| Role | Read | Write content | Change content types | Manage members |
|---|---|---|---|---|
| Owner | yes | yes | yes | yes |
| Developer | yes | yes | yes | no |
| Editor | yes | yes | no | no |
| Publisher | yes | yes | no | no |

- Any member lists the members (`GET /api/v1/projects/{project}/members`) and may leave; people whose role has
  `members.manage` (owners, by default) change roles (`PUT`) and remove people (`DELETE …/members/{identity}`).
- Since step R.2 (D-058), owners define roles of their own. `/api/v1/projects/{project}/roles` (a project's roles, not an environment's) lists and reads them (any member: anyone can
  see what a role means before it's given to them), creates them (`POST`), changes them (`PUT` with `If-Match:
  "v<version>"`; a stale version is 409 `stale_role` with the current `ETag`) and deletes them (`DELETE` with
  `If-Match`). Writes need `roles.manage`, checked against the caller's role as it is now. Migration 0018 adds a
  version to each role.
- **Nobody hands out more than they hold** (`Policy::covers`): creating, changing or deleting a role needs every
  power of the role before and after; giving someone a role, or removing them, needs `members.manage` and every
  power of both their old role and the new one, re-checked under the owner lock; inviting needs every power of the
  role offered. So a role that manages members but can't change content types can't demote a developer, promote
  itself to owner, or remove an owner.
- **The owner role is fixed**: the API answers 409, and migration 0018's triggers refuse any update or delete of it
  or its permissions, even by a direct write as the application's login.
- **A role in use can't be deleted** (409 `role_in_use`, naming how many members and pending invitations): members
  keep 0017's foreign key; invitations don't, because accepted, revoked and expired ones are history, so inviting
  holds a shared advisory lock on the role instead (in memory, not on the role's row, which hundreds of invitations
  a second would contend for) and deleting takes it exclusively and counts pending invitations.
- Since step R.3, **a permission can be narrowed**: `"fields": [...]` on `content.read`, `content.create` and
  `content.update`, and `"condition": "own"` (entries the member created) on `content.read`, `content.update` and
  `content.delete`; anything else is 422 (`fields_not_applicable`, `invalid_fields`, `condition_not_applicable`,
  `unknown_condition`), and migration 0021's checks refuse it in the database too. A role holds one permission per
  action, subject and condition (the domain merges field lists), so "titles of any article" and "every field of their
  own articles" sit side by side.
- `Policy::fields(action, collection, own)` decides per entry: every permission that applies adds its fields, one
  without a list allows all. `Policy::allows` now means every field of every entry, so a narrow permission never
  grants collection-wide power, and `covers` is exact for fields and `own`.
- Entries enforce it (`colophon_api::visibility`): a read leaves out what the role can't read (`hidden_fields`) and
  marks what it can read but not change (`locked_fields`); an entry it can't read at all is 404; a list holds only
  the caller's own entries when that is all the role may read, with titles blank where the title field is hidden;
  revisions and history the same. A title search (`?q=`) looks only where the role reads titles: its own entries
  when it reads only theirs, and none (403 `titles_not_readable`) when it reads none, so which entries match can't
  confirm a guess at a hidden title (found by the docs rewrite, fixed with a test that failed before). Creating may only set fields the role may create. Saving puts back every field
  the saver can't read exactly as stored, refuses one sent with a different value, and refuses any change to a
  field the role can't change (422 `field_not_permitted`, pointing at the field), measured against the immutable
  revision the editor started from. A publish request may pin an entry only if the role may change it.
- The Studio's editor leaves hidden fields out and never sends them, shows locked ones disabled, and says which; a
  role's page lists limited permissions in words and keeps them when the grid is saved.
- Creating a role queues on the project's owner role row (`FOR NO KEY UPDATE`), so the count can't race past 100
  roles a project; changing or deleting one locks only that role.
- A member change locks the owners, the caller and the target in one statement, in identity order, so changes to
  one project's members queue and can't deadlock. An owner skips reading role permissions (the owner role holds every
  one, and 0018 keeps it so); anyone else's change reads the roles involved in one more query. Measured on the
  reference laptop with `perf/k6/members.js` at 1,000 requests a second (100 role changes and 100 invitations a
  second on one project): uncontended, a role change takes 4.2 ms against 4.0 ms before R.2; under that load,
  medians of 9 to 23 ms for role changes and 8 to 15 ms for invitations, against 27 to 47 and 7 to 30 before. A first
  version that read each role separately under the lock took 0.8 ms more per change, and the queue it fed grew
  medians to 190 and 440 ms. A changed role holds from its holders' next request; a request already running acts on the permissions
  it read, as content writes act on the role read when their transaction began.
- `GET /api/v1/projects/{project}` returns the caller's role name and permissions, and the Studio decides what to
  offer with `may(action, collection)`, the same rule as `Policy::allows`, instead of comparing role names. Settings
  shows every role and edits one as a grid in plain words (See, Create, Edit, Approve, Publish, Change fields; for
  "Everything" and each content type) plus the whole project's powers.
- A project never loses its last owner: `colophon_db::members` locks the owners `FOR UPDATE` before changing one, and
  the deferred trigger `memberships_keep_an_owner` (migration 0005) refuses any commit that would leave none. The API
  answers 409.
- An owner's role is re-checked under that lock before a member change, and under a share lock on their own membership
  before creating or revoking an invitation: an owner demoted while their request runs gets 403, not the old role's
  rights. Content writes act on the role read when their transaction began.
- The cost of that lock: changes to one project's members queue. In `perf/k6/members.js`, 100 role changes a second
  on one project wait a median 34 ms (p99 81 ms), and invitations, which take a share lock on their owner's row, wait
  behind them; at 10 role changes a second, they take 4.5 ms like any other request.
- Publishers can't publish yet: publishing arrives in P2, and with it the difference between editor and publisher.

### Invitations (spec §04)
- Single-use, expiring after a configurable period (default 72 hours).
- Bound to the authenticated, verified identity that accepts them.
- The invitee sees the project name, role and capabilities before accepting.
- Built in step 1.7 (D-042):
  - owners create (`POST …/invitations` with `{"role", "label"}`), list and revoke them;
  - the answer that creates one is the only place its link, `/studio/invite#<token>`, appears. Only the token's
    SHA-256 hash is stored, and the fragment never reaches a server;
  - `POST /api/v1/invitations/lookup` shows the holder the project's name, the role and who invited them, and
    `POST /api/v1/invitations/accept` joins. Both take the token in the body;
  - an expired, revoked or used invitation is 404; someone already in the project gets 409, and the invitation stays
    unused;
  - the Studio's `/studio/invite` keeps the token in this tab's session storage across sign-in and takes it out of
    the address bar.

### Machine credentials (P5)
- Built early, for people (step 1.6, D-041): **personal access tokens for the CLI**, issued by a device sign-in that a
  signed-in person approves at `/studio/device`. 30 days, hashed, revocable, acting with the person's memberships;
  `Authorization: Bearer` is accepted on every route. Scoped machine credentials for scripts and agents remain P5.
- Scoped to project, environment, collections and actions.
- Expiring, with quotas, stored hashed and compared in constant time, as session tokens are.
- The CLI signs in through a browser or device-authorization flow.
- A broad administrator token never appears in a template or frontend bundle.
- A **kill switch** revokes a principal and prevents new work; actions it already completed stay auditable.

### The policy function
- `authorize(principal, action, resource) -> Decision`, deny by default.
- Its first form, built in step 1.7: `colophon_domain::policy::allows(role, action)` over `Read`, `WriteContent`,
  `ChangeContentTypes` and `ManageMembers`; step 2.1 adds `Review` and step 2.2 `Publish`, both for owners and
  publishers (D-044, D-045); step 2.3 `ManageDestinations`, for owners and developers (D-046).
- Since step R.1 (D-058), roles are data. `roles` and `role_permissions` (migration 0017) hold each project's roles;
  a permission is an action (`content.read`, `content.create`, `content.update`, `content.delete`, `content.review`,
  `content.publish`, `schema.change`, `members.manage`, `roles.manage`, `destinations.manage`, `tokens.manage`), a
  subject (`*` or one collection's slug), and a field list and `own` condition stored for step R.3.
  `colophon_seed_roles` gives every project the four built-in roles with D-042's powers, and owners now also hold
  `roles.manage` and `tokens.manage`, which no route uses yet.
- `scope::begin_as_member` reads the caller's membership and their role's permissions in one statement (a lateral
  aggregate, one pass over `role_permissions`), so every `MemberTransaction` and `ProjectTransaction` carries a
  `Policy`. A stored action or condition the code doesn't know is `DbError::Corrupt`, never a skipped permission.
- `Policy::allows(action, collection)` is the one evaluator, with no I/O: deny unless a permission for that action
  names `*` or that collection. Every handler asks it through `members::require`: entries check `content.read`,
  `content.create` or `content.update` for their own collection; a changeset checks `content.update`, and its review
  `content.review`, for every collection it pins; publishing checks `content.publish` for each, after locking them;
  content types check `schema.change` for theirs. A collection-wide check passes `None` and holds only for `*`.
  Project-wide actions (members, roles, destinations, tokens) can't be granted for one collection.
- Principals other than people and resource-level checks arrive with machine credentials (R.4, P5).
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
| A7 | Bootstrap runs once and is then disabled, whether by the command or a setup code (D-061); the development identity never runs on a public bind. |
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
- **Since step 3.7 (D-061):**
  - `crates/colophon-db/tests/setup.rs`: only a hash is stored and the application login can neither read the
    codes nor write `installation`, `administrators` or the attempts; a code makes one administrator once and then
    nobody else; an expired code is dead; lifetimes outside a minute to a day are refused; at most ten codes are
    live; after a bootstrap nothing is issued, attempted or claimed; five attempts a person and twenty in all, with
    refused attempts unrecorded and room again after fifteen minutes; two codes claimed at the same moment make one
    administrator;
  - `crates/colophon-server/tests/setup.rs`, against the real binary on a database nobody bootstrapped: the code
    printed to standard error makes the first administrator, typed in lower case with spaces; wrong and malformed
    codes get 422, the used code 409 for anyone else, and a second start prints nothing; signed out is 401 and a
    cookie without CSRF 403; an expired code 422; the sixth attempt 429 with `Retry-After`, even with the right code,
    while someone else within their limit succeeds; the code appears exactly once in all the process's output
    (its banner) and in no response or structured log line, at `debug`;
  - `crates/colophon-server/tests/sign_in.rs`: an account made through `/auth/login?start=create` on Keycloak's
    sign-up form signs in with no projects, isn't an administrator, and can't create a project;
  - unit tests: codes are four groups of Crockford characters and read back however they are typed; only an exact
    hash matches, comparing every live code; the banner names the code once, the Studio and the expiry in UTC.
- **Since step 1.8 (D-043):**
  - `crates/colophon-db/tests/administrators.rs`: a bootstrap before the person's first sign-in, which then finds
    the same identity; again for the same person changes nothing, for anyone else it's refused; the installation row
    survives delete, update and truncate as the table owner; two bootstraps at the same moment make one
    administrator; the database refuses a non-administrator's project; the application login can't insert an
    administrator or an installation;
  - `crates/colophon-server/tests/bootstrap.rs`, against the real binaries: `/me` says dev-owner is an administrator
    and dev-editor isn't; dev-editor's project is refused with 403 and dev-owner's created; `colophon admin bootstrap`
    exits 0 for dev-owner again, 4 for dev-editor, 2 for a plain-http issuer off localhost;
  - `apps/studio/tests/server/projects.spec.ts` and the testbed's session spec: the administrator creates a project
    in the Studio, others see how to join one instead, and the testbed shows the refusal;
  - `perf/k6/projects.js`: refused creations and `/me` for both people, at least 1,000,000 requests per full run,
    checking that no refusal created anything.
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
- **Since step 1.7 (D-042):**
  - `crates/colophon-domain`: the policy table, every role against every action (since R.1, in every collection and
    collection-wide, plus collection-scoped permissions and name round trips);
  - `crates/colophon-db/tests/roles.rs` (R.1): a new project's seeded roles equal `Policy::builtin`, the creator's
    transaction carries the owner's policy, and a membership naming an unknown role is refused; (R.2) a role is
    created, changed only at its version, holds from its holder's next transaction, and is deleted only once unused;
    the owner role refuses a direct write; a revoked invitation doesn't hold a role; a member manager can't demote a
    developer, promote himself, or remove an owner, but can give his own role;
  - `crates/colophon-server/tests/permissions.rs` (R.2), against the real binary: a role the owner defines decides
    what its holder may do from their next request; `stale_role`, `role_in_use`, the fixed owner role; unknown
    actions, project-wide actions on a collection, field lists and conditions refused; escalation refused over HTTP;
  - `apps/studio/tests/server/permissions.spec.ts` and `apps/testbed/tests/roles.spec.ts` (R.2): make a role, tick
    what it may do, lose a stale save in a second tab, invite someone who holds exactly it, a held role that can't be
    deleted, the fixed owner role, axe on every Studio page;
  - `perf/k6/roles.js` (R.2): a custom role's member listing articles, their permissions, role lists and versioned
    edits;
  - `crates/colophon-db/tests/members.rs`: an invitation joins once and shows its project only to its holder;
    revoked and expired invitations are dead; the last owner can be neither demoted nor removed; two owners demoting
    each other at the same moment leave one owner; an owner demoted while their request runs can neither promote nor
    remove anyone; another project's owner can neither see nor revoke these
    invitations;
  - `crates/colophon-server/tests/members.rs`, against the real binary with two people signed in through Keycloak:
    invite, look up, accept once; an owner can't join their own project twice; revoked links are gone; an editor
    writes content but can't change types, invite or promote themselves; a developer can change types; the last
    owner can't step down until there's a second; leaving hides the project;
  - `apps/studio/tests/server/members.spec.ts` and `apps/testbed/tests/members.spec.ts`: the same story in two real
    browsers, with axe on every Studio page;
  - `perf/k6/members.js`: member lists, invitation lookups, role changes and new invitations, at least 1,000,000
    requests per full run.

## Build order

1. **P1:** OIDC with Keycloak, sessions, CSRF/Origin and cross-project denial for projects (step 1.1, done); roles,
   invitations and the policy function (step 1.7, done); the owner bootstrap (step 1.8, done).
2. **R:** roles as data (R.1, done), custom roles and the Studio's role editor (R.2, done), field-level permissions and `own`
   conditions (R.3, done), API tokens and the audit log (R.4); docs/ROADMAP.md, D-058.
3. **P2:** approval permissions and self-review recording.
4. **P4:** preview capabilities.
5. **P5:** machine credentials, device-authorization login, quotas, kill switch.

## Open decisions

- Whether a hidden resource answers 404 everywhere, or 403 for members of the same project.
