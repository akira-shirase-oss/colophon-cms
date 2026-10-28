Colophon uses two sets of words for the same things. The Studio and the editor pages use **plain words**, adopted on 2026-10-27 in decision D-059 so that someone who isn't a developer can run Colophon without help; the API, the command line and the developer pages keep the **technical terms**. This page puts them side by side.

Since step 3.7 the Studio takes every word from one glossary file (`apps/studio/src/lib/words.ts`), and a test fails if a technical word shows outside Settings and its *Technical details* drawer. Where both columns hold the same word, there is no separate plain word. Terms marked *on a branch* name things built and tested on a branch and not merged yet; the Studio may not show them.

## Terms

| Plain word | API and CLI | What it means | More |
|---|---|---|---|
| administrator | administrator | The person who can create projects in an installation. The first is made once, from a terminal, with `colophon admin bootstrap`. | [Roles and permissions](/concepts/roles-and-permissions/) |
| allowed form | pattern (`"pattern"` on a text field) | The form a text value must match whole, such as three capital letters, a hyphen and four digits. On a branch; the Studio's word comes from its builder on branch `studio/dashboard`. | [Content modelling](/reference/content-modelling/#patterns) |
| API token | API token (`colophon_api_…`) | A key a script, a build or an agent uses instead of a person signing in: one project, its own permissions, its own name on what it writes, revoked at once. On a branch. | [API tokens](/reference/api/tokens/) |
| article, entry | entry | One piece of content in a collection, such as an article: a draft and its history of versions. | [Drafts and versions](/concepts/drafts-and-versions/) |
| audit log | audit log (`audit_events`) | The record of every change to who may do what: roles, members, invitations and API tokens. Nobody can change or delete what it holds. On a branch. | [The audit log](/reference/api/tokens/#the-audit-log) |
| check the site | probe (`kind: probe`) | A website connection that fetches a page after each release and reads which release it was built from. | [Receipts and probes](/concepts/receipts-and-probes/) |
| clear the CDN | CDN purge (`kind: cdn`) | A website connection that asks a CDN to drop the copies a release changed. Accepted isn't proof they are gone. | [Unpublish and revocation](/concepts/unpublish-and-revocation/) |
| collection | collection (`articles`) | All the entries of one content type. Every project starts with `articles`. | [Content types](/concepts/content-types/) |
| content type | content type (`content-types`) | The versioned list of fields every entry of a collection must fit; changed only in ways that keep stored entries valid. | [Content types](/concepts/content-types/) |
| delivery API, public API | `/content/v1` | The public, read-only API that serves only published content, at the latest release or any past one. | [Caching and delivery](/concepts/caching-and-delivery/) |
| developer | `developer` | Built-in role: writes content, changes content types and manages website connections. | [Roles and permissions](/concepts/roles-and-permissions/) |
| draft | draft | An entry's newest version, the one being edited. Readers never see drafts. | [Drafts and versions](/concepts/drafts-and-versions/) |
| editor | `editor` | Built-in role: writes and edits content. | [Roles and permissions](/concepts/roles-and-permissions/) |
| environment | environment (`production`) | One copy of a project's content and releases. Every project has one today, `production`. | [Releases and generations](/concepts/releases-and-generations/) |
| field | field | One named value in a content type: text, rich text, number, yes/no, choice, date-time or URL name, with its limits; on a branch also email, web address, date, time, JSON, Markdown, colour, references and structures. | [Content modelling](/reference/content-modelling/#field-types) |
| group of fields | `object` field | Fields kept together inside one field, such as a hero with a heading and an image text. On a branch. | [Content modelling](/reference/content-modelling/#objects-lists-and-blocks) |
| invitation | invitation | A link that grants one role in one project, works once, expires after 72 hours and can be revoked. | [Roles and permissions](/concepts/roles-and-permissions/) |
| label | `label`: `published`, `pending`, `verified`, `needs_attention` | A release's one word on how far the evidence goes: *Published in CMS*, *Website update pending*, *Verified at N locations* or *Needs attention*. | [Receipts and probes](/concepts/receipts-and-probes/) |
| list | `list` field | Several values of one kind in one field, such as tags; at most 100. On a branch. | [Content modelling](/reference/content-modelling/#objects-lists-and-blocks) |
| observation | observation | One look a probe took: when, from which location, which release the answer named, and its HTTP status. | [Receipts and probes](/concepts/receipts-and-probes/) |
| owner | `owner` | Built-in role with every permission, including members. A project always keeps at least one. | [Roles and permissions](/concepts/roles-and-permissions/) |
| page builder | `blocks` field | An ordered list of blocks, each one of the kinds the field offers, such as text, a quote or a link. On a branch. | [Content modelling](/reference/content-modelling/#objects-lists-and-blocks) |
| permission | permission (action and subject) | Something a role allows, such as publishing, for every collection or one, and optionally only some fields or only the member's own entries. | [Roles and permissions](/concepts/roles-and-permissions/) |
| project | project | One website's content, people and settings, isolated from every other project by the database. | [Roles and permissions](/concepts/roles-and-permissions/) |
| publish request | changeset | The exact versions to publish or take down, reviewed and published together. An approval counts only for those versions. | [Publish requests](/concepts/publish-requests/) |
| publisher | `publisher` | Built-in role: writes, reviews and publishes. | [Roles and permissions](/concepts/roles-and-permissions/) |
| rebuild the site | webhook (`kind: webhook`) | A website connection that sends each release to an address, as a `release.committed` POST, so the site can rebuild. | [Webhooks](/reference/webhooks/) |
| receipt | receipt (`GET {P}/releases/{generation}`) | A release's label, each website connection's part in delivering it, and every observation. | [Receipts and probes](/concepts/receipts-and-probes/) |
| reference, referenced by | `reference` field, `referenced_by` | A field that names other entries, which must exist; an entry's page says which entries name it. On a branch. | [References](/reference/content-modelling/#references) |
| release, and the time it went live | release | One publish: a numbered, permanent step in the website's history. Everything in it went live together. | [Releases and generations](/concepts/releases-and-generations/) |
| release number | generation (`Colophon-Generation`) | A release's number in its environment. It only moves forward, and every public answer names the one it was read at. | [Releases and generations](/concepts/releases-and-generations/) |
| rich text | `colophon/rich-text` v1 | Formatted text, stored as JSON the server checks on every save, never as HTML. | [Rich-text format](/reference/rich-text/) |
| role | role | What a member may do in a project. Four are built in, and owners can define more. | [Roles and permissions](/concepts/roles-and-permissions/) |
| schedule | schedule (`POST {P}/changesets/{changeset}/schedule`) | A time, in a time zone, when an approved publish request publishes itself, as the person who set it. | [Scheduled publishing](/concepts/scheduled-publishing/) |
| setup code | setup code (`POST /api/v1/setup`) | A one-time code a new installation's server prints to its terminal; whoever enters it, signed in, becomes the first administrator. | [Onboarding and plain words](/concepts/onboarding/) |
| single type | `"single": true` | A content type with one entry only, such as a site's settings. On a branch. | [Single types](/reference/content-modelling/#single-types) |
| take down | unpublish | Removing an entry from the website, through a reviewed release. | [Unpublish and revocation](/concepts/unpublish-and-revocation/) |
| unique field | `"unique": true` | A field whose value belongs to one entry, such as an email address or a product code, compared exactly as typed. On a branch. | [Unique fields](/reference/content-modelling/#unique-fields) |
| URL name | slug | The part of a web address that names one entry, unique in its collection. | [Content types and URL names](/concepts/content-types/) |
| version | revision | One save of an entry. Never changed or deleted afterwards. | [Drafts and versions](/concepts/drafts-and-versions/) |
| website connection | destination | Somewhere each release is announced: rebuild the site, clear the CDN, or check the site. | [Outbox and worker](/concepts/outbox-and-worker/) |

## Developer terms

These have no plain word: an editor never needs them.

| Term | What it means | More |
|---|---|---|
| canonical form | The one way Colophon writes a value or a definition, the exact bytes it stores and hashes, so the same thing always has the same hash. | [Content modelling](/reference/content-modelling/#the-stored-form) |
| `ETag`, `If-Match` | An answer's `ETag` names the version it returned; a save sends it back in `If-Match`, so a save built on an older version is refused with `409` instead of overwriting. | [Saves and conflicts](/concepts/saves-and-conflicts/) |
| fault case | A failure a test causes on purpose, named F01, F02 and so on, such as killing the server mid-save, with what must still be true afterwards. | [Crash tests](/operations/crash-tests/) |
| idempotency key | An `Idempotency-Key` header on a write. Retried with the same key, the request is recognised and done once; reused for a different request, it is refused. | [Saves and conflicts](/concepts/saves-and-conflicts/) |
| job | One delivery task: one website connection, at one release. | [The outbox and the worker](/concepts/outbox-and-worker/) |
| lease | A worker's time-limited claim on a job, 30 seconds by default. A result from a worker whose lease ran out is refused. | [The outbox and the worker](/concepts/outbox-and-worker/) |
| MCP server | `colophon-mcp`: tools an AI agent uses through the Model Context Protocol to read content, write drafts and ask for review, with an API token; never to approve or publish. On a branch. | [The MCP server](/reference/mcp/) |
| outbox | The jobs a release commits in the same transaction as itself, so a release is never announced without being made, or made without being announced. | [The outbox and the worker](/concepts/outbox-and-worker/) |
| seed | The number that makes one simulator run reproducible: the same seed replays the same run, step for step. | [The deterministic simulator](/concepts/simulator/) |
| simulator | `colophon-sim`: thousands of seeded runs of colliding editors, publishers, crashes and network faults, checking every invariant. | [The deterministic simulator](/concepts/simulator/) |
| tombstone | The record that an entry was taken down at a release. From then on the public API answers `410` for it, at that release and at every older one. | [Unpublish and revocation](/concepts/unpublish-and-revocation/) |
| worker | The background part of `colophon-server` (`--role worker`, or `all`) that claims and does the jobs. | [The worker](/operations/worker/) |
| `{P}` | Short for `/api/v1/projects/{project}/environments/{environment}`, the start of every content route. | [API conventions](/reference/api/) |
