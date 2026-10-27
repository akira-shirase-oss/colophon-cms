Every route `colophon-server` answers today, from its router (`crates/colophon-api/src/lib.rs`). Below, `{P}` stands for `/api/v1/projects/{project}/environments/{environment}`. "Member" means any member of the project; the [policy table](/concepts/roles/) decides the rest. Requests that change something need the headers in the last column; see [conventions](/reference/api/).

## Health and sign-in

| Method and path | Who | Notes |
|---|---|---|
| `GET /livez` | anyone | The process responds. |
| `GET /readyz` | anyone | Readiness: `ready`, `degraded`, `starting`, `not_ready`, `draining`. |
| `GET /auth/login` | anyone | Starts OpenID Connect sign-in; `?return_to=` a path on this server. |
| `GET /auth/callback` | the provider | Finishes sign-in; a sign-in attempt works once, within 10 minutes. |
| `POST /auth/logout` | signed in | Ends the session, or revokes the bearer token that sent it. |
| `GET /me` | signed in | You, your projects, your `subject` and whether you are an `administrator`. |
| `POST /auth/device` | anyone | Starts a device sign-in: a secret to poll with, a code, and the approval link. |
| `GET /auth/device/{code}` | browser session | What is asking to sign in, for the approval page. |
| `POST /auth/device/approve` | browser session | `{"user_code"}`: lets the device sign in as you. Bearer tokens can't approve. |
| `POST /auth/device/token` | the device | Polls with the secret: pending, then a token exactly once. |

## Projects, members and invitations

| Method and path | Who | Notes |
|---|---|---|
| `GET /api/v1/projects` | signed in | Your projects. |
| `POST /api/v1/projects` | administrators | `{"slug", "name"}`; creates `production` and makes you its owner. A taken slug is `409`. |
| `GET /api/v1/projects/{project}` | member | |
| `GET /api/v1/projects/{project}/members` | member | |
| `PUT /api/v1/projects/{project}/members/{identity}` | owners | `{"role"}`. Never leaves the project without an owner (`409`). |
| `DELETE /api/v1/projects/{project}/members/{identity}` | owners, or yourself to leave | |
| `GET /api/v1/projects/{project}/invitations` | owners | Pending invitations. |
| `POST /api/v1/projects/{project}/invitations` | owners | `{"role", "label"}`; returns the link once. |
| `DELETE /api/v1/projects/{project}/invitations/{invitation}` | owners | Revokes it. |
| `POST /api/v1/invitations/lookup` | signed in | `{"token"}`: the project, role and inviter. |
| `POST /api/v1/invitations/accept` | signed in | `{"token"}`: joins. Already a member is `409`, and the invitation stays unused. |

## Content types

| Method and path | Who | Headers | Notes |
|---|---|---|---|
| `GET {P}/content-types` | member | | The current version of each. |
| `POST {P}/content-types` | owners, developers | `Idempotency-Key` | `{"slug", "definition"}`. |
| `GET {P}/content-types/{slug}` | member | | With `ETag: "v<version>"`. |
| `PUT {P}/content-types/{slug}` | owners, developers | `If-Match`, `Idempotency-Key` | A new version; only [safe changes](/concepts/content-types/), else `422` `needs_migration`. |

## Entries and revisions

| Method and path | Who | Headers | Notes |
|---|---|---|---|
| `GET {P}/collections/{collection}/entries` | member | | Newest first; `limit`, `cursor`, `q` (title search). Each with its live state. |
| `POST {P}/collections/{collection}/entries` | member | `Idempotency-Key` | The content as JSON; `201` with the draft and its `ETag`. |
| `GET {P}/collections/{collection}/entries/{entry}` | member | | The draft, with `ETag: "<revision id>"`. |
| `PUT {P}/collections/{collection}/entries/{entry}` | member | `If-Match`, `Idempotency-Key` | Saves a new revision on the one named; stale is `409`. |
| `GET {P}/collections/{collection}/entries/{entry}/revisions` | member | | The history, newest first. |
| `GET {P}/collections/{collection}/entries/{entry}/revisions/{revision}` | member | | One revision's content. |
| `POST /api/v1/rich-text/validate` | signed in | | A rich-text document's canonical form, or a `422` naming the rule and where. Stores nothing. |

## Changesets, releases and receipts

| Method and path | Who | Headers | Notes |
|---|---|---|---|
| `GET {P}/changesets` | member | | The newest 100, each with its status. |
| `POST {P}/changesets` | member | `Idempotency-Key` | `{"title", "entries": [ids], "unpublish": [ids]}`, 1 to 100 entries. |
| `GET {P}/changesets/{changeset}` | member | | Items, status and reviews, with `ETag: "sha256:…"`. |
| `POST {P}/changesets/{changeset}/submit` | member | `If-Match` | Sends it for review. |
| `POST {P}/changesets/{changeset}/refresh` | member | `If-Match` | Re-pins entries that moved on; gives a new hash. |
| `POST {P}/changesets/{changeset}/reviews` | owners, publishers | `If-Match` | `{"decision": "approve" or "request_changes", "comment"}`. |
| `POST {P}/changesets/{changeset}/publish` | owners, publishers | `If-Match`, `Idempotency-Key` | `{"base_generation"}`; `409` `generation_moved` if another release came first. |
| `GET {P}/releases` | member | | Newest first, each with its label and purge state; `Colophon-Generation` is the latest. |
| `GET {P}/releases/{generation}` | member | | The receipt: label, each destination's part and every probe observation. |

## Destinations and jobs

| Method and path | Who | Notes |
|---|---|---|
| `GET {P}/destinations` | member | |
| `POST {P}/destinations` | owners, developers | `{"kind", "name", "url", "location"}`; `kind` is `webhook` (the default), `cdn` or `probe`. See [webhooks](/reference/webhooks/). |
| `PUT {P}/destinations/{destination}` | owners, developers | `{"enabled": true or false}`. |
| `GET {P}/jobs?generation=N` | member | The worker's jobs for a release: state, attempts, last status and error. |

## Public delivery

| Method and path | Who | Notes |
|---|---|---|
| `GET /content/v1/{project}/{environment}/{collection}` | anyone | Published entries; `generation`, `limit`, `cursor`. |
| `GET /content/v1/{project}/{environment}/{collection}/{entry}` | anyone | One published entry; `generation`. |

See [Delivery API](/reference/delivery-api/).

## The Studio

With `COLOPHON_STUDIO_DIR` set, the server also serves the Studio's files at `/studio/`, and its `200.html` for any other path under it, with security headers on every response. Anything else answers `404` with `route_not_found`.
