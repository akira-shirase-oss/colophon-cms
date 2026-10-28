<script>
	import Status from '#lib/components/Status.svelte';
</script>

A **changeset** (a *publish request* in the Studio's plain words) is the exact set of revisions someone wants to publish or take down. A review approves that exact set: change anything in it and the approval no longer counts. "Approved" means "these bytes were looked at", not "this list of entries was looked at at some point".

## Pinned revisions and one hash

Each item pins one entry at one revision. The revision row carries its content hash, its collection and the content-type version it was validated against (with that version's definition hash), so the changeset's hash can always be recomputed: SHA-256 over a canonical form, `colophon_domain::changeset::canonical`. That is a version line, `colophon/changeset v1`, then one line per item, sorted by entry id: the entry, the revision, the collection, the content hash, the content-type version and its definition hash. The hash is written `sha256:<hex>` and served as the changeset's `ETag`. Two changesets with the same items have the same hash, whatever order the items were added in.

An item can also be an **unpublish**, which pins the revision that is currently published, so the reviewer approves taking down exactly what readers see ([Unpublish and revocation](/concepts/unpublish-and-revocation/)). Its line carries the word `unpublish` after the entry id, where a publish line has a revision id, so every changeset hashed before unpublishing existed kept its hash.

A changeset holds 1 to 100 entries in all (`entries` and `unpublish` together, each entry in one list only), and creating one takes an `Idempotency-Key`.

## States

`colophon_domain::changeset::status` decides the state from the items, the reviews and the clock:

| State | When |
|---|---|
| `open` | Created, not submitted. |
| `stale` | An entry moved on after it was pinned: edited, or, for an unpublish, published anew or taken down already. Refreshing re-pins it, which changes the hash. |
| `awaiting_review` | Submitted, with no decision on the current hash. |
| `changes_requested` | The latest decision on the current hash asked for changes. |
| `approved` | The latest decision on the current hash approved it; `self_review` says whether the person who submitted it approved it. |
| `approval_expired` | Approved more than seven days ago: it needs reviewing again. |
| `published` | Published as the release with this generation. A changeset publishes once and stays the record of what that release contained. |

Only an `approved` changeset can be published; every other state is refused with `409` and a message saying what it needs instead.

## Every action names the hash it was looking at

`submit`, `refresh`, `reviews` and `publish` all require `If-Match` with the changeset's hash. If the changeset changed since the caller loaded it, the action is refused with `409`, `details.reason` `stale_changeset`, and the current hash in the `ETag`. So a reviewer can never approve a version they didn't see, even in a race with an edit in another tab.

Only decisions on the current hash count, and the latest wins. "Latest" is not a timestamp, because two decisions can share one: each decision takes a per-changeset sequence number under a lock on the changeset's row, which refreshes and reviews take too. The first version of this used timestamps; its end-to-end test caught decisions in the same second applied in the wrong order. A changeset shows its newest 100 reviews; since a refresh always gives a new hash, every review on the current hash is among them.

## Who may do what

Each action asks the [evaluator](/concepts/roles-and-permissions/) about every collection the changeset touches:

| Action | Needs, in every collection it pins | By default |
|---|---|---|
| create, submit, refresh | `content.update` | every built-in role |
| review (approve or ask for changes) | `content.review` | owners and publishers |
| publish | `content.publish` | owners and publishers |

A custom role can hold them for some collections and not others (R.2). Since R.3, a publish request may include an entry only if the caller's role may change it: anyone's, or their own where that is all it may change. <Status done="R.3" />

**Self-review** is allowed and recorded: an approval by the person who submitted the changeset counts, and the changeset says `self_review: true`. That is the spec's default (§06: "the default permits a single operator to review their own work, visibly recorded"). The rule for requiring a different reviewer is in the domain (`require_different_reviewer`, under which a self-approval doesn't count) and is off for every project: one constant, `colophon_db::publish::REQUIRE_DIFFERENT_REVIEWER`, is `false` until project settings exist, and a project read says so as `self_review: true`, which the Studio's one-click publishing follows. A per-project setting for it has no step yet. <Status planned="not scheduled" />

## Publishing it

Publishing an approved changeset makes a release, in one transaction, on the release the publisher last saw ([Releases and generations](/concepts/releases-and-generations/)). Since step 3.6, publishing first takes the content-type lock of every collection in the changeset, shared and in order, so no content type changes between reading the items and committing; and a release that would publish a URL name another entry still publishes is refused whole with `409 slug_taken`. The publish is one routine, `colophon_db::publish::publish`, which a person's publish and a scheduled one both run. <Status done="3.6" />

## Built on top, and planned

- **One-click publishing.** In the Studio, publishing one entry is one button on the entry: it creates the publish request, sends it for review, and approves and publishes it at once when the project lets people review their own work and the role may approve and publish, or stops after asking for review otherwise. It is still a changeset, a recorded review and a release underneath ([Onboarding and plain words](/concepts/onboarding/)). <Status done="3.7" />
- **Scheduling.** An approved changeset can be given a time to publish, in a time zone, and is checked again when it falls due ([Scheduled publishing](/concepts/scheduled-publishing/)). <Status done="D-060" />
- Review stages, assignees and comments on top of changesets. <Status planned="E" />
- Asset hashes in the canonical form, with media, as a new format version. <Status planned="P4" />

## How it is tested

`crates/colophon-server/tests/changesets.rs` runs it end to end: an approval bound to the hash, an edit making the changeset stale, a refresh re-pinning it, decisions in the same second ordered correctly. The domain's tests cover the canonical form and every state; `crates/colophon-db/tests/changesets.rs` runs against PostgreSQL. The Studio's tests run review in two tabs (`apps/studio/tests/server/changesets.spec.ts`), the testbed covers the API (`apps/testbed/tests/changesets.spec.ts`), and `perf/k6/changesets.js` is its load scenario.

## Design

[Module 01, domain](/design/modules/01-domain/) and [module 04, api](/design/modules/04-api/); decisions D-029, D-044, D-049 and D-060 in the [decisions](/design/decisions/). The editor's side is [Publish requests and review](/editors/review/); the routes are in [Publish requests and reviews](/reference/api/publish-requests/).
