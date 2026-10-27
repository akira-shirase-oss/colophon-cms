A **changeset** (a *publish request* in the editor guide) is the exact set of revisions someone wants to publish. A review approves that exact set: change anything in it and the approval no longer counts. "Approved" means "these bytes were looked at", not "this list of entries was looked at at some point".

## Pinned revisions and one hash

Each item pins one entry at one revision. The revision row carries its content hash, collection and content-type version, so the changeset's hash can always be recomputed: SHA-256 over a canonical form (a version line, then one line per item, sorted by entry id). It is written `sha256:<hex>` and served as the changeset's `ETag`.

An item can also be an **unpublish**, which pins the revision that is currently published, so the reviewer approves taking down exactly what readers see. Its line carries the word `unpublish`, which left the hash of every earlier changeset unchanged when unpublishing arrived.

## States

`colophon_domain::changeset::status` decides the state from the items and the reviews:

| State | When |
|---|---|
| `open` | Created, not submitted. |
| `stale` | An entry has a newer revision than the pinned one. Refreshing re-pins it, which changes the hash. |
| `awaiting_review` | Submitted on the current hash. |
| `changes_requested` | The latest decision on the current hash asked for changes. |
| `approved` | The latest decision on the current hash approved it; `self_review` says whether the author approved their own. |
| `approval_expired` | Approved more than seven days ago. |

## Every action names the hash it was looking at

`submit`, `refresh`, `reviews` and `publish` all require `If-Match` with the changeset's hash. If the changeset changed since the caller loaded it, the action is refused with `409`, `details.reason` `stale_changeset`, and the current hash in the `ETag`. So a reviewer can never approve a version they didn't see, even in a race with an edit in another tab.

Only decisions on the current hash count, and the latest wins. "Latest" is not a timestamp, because two decisions can share one: each decision takes a per-changeset sequence number under a lock on the changeset's row, which refreshes and reviews take too. The first version of this used timestamps; its end-to-end test caught decisions in the same second applied in the wrong order.

## Who may do what

Any member can create a changeset (1 to 100 entries, with an `Idempotency-Key`) and submit it. Owners and publishers review and publish. Self-review is allowed and recorded; the rule that requires a different reviewer exists in the domain and is off for every project until project settings exist.

## How it is tested

`crates/colophon-server/tests/changesets.rs` runs it end to end: an approval bound to the hash, an edit making the changeset stale, a refresh re-pinning it, decisions in the same second ordered correctly. The Studio's tests run review in two tabs. `perf/k6/changesets.js` is its load scenario.

## Design

[Module 01, domain](/design/modules/01-domain/) and [module 04, api](/design/modules/04-api/); decisions D-029, D-044 and D-049 in the [decisions](/design/decisions/). On GitHub: [docs/DECISIONS.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/DECISIONS.md).
