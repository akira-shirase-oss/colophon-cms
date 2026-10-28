<script>
	import Status from '#lib/components/Status.svelte';
</script>

A save is a request to make a new revision on top of a particular one. If that particular one is no longer the draft, because someone else saved in between, the save is refused. Nothing is merged and nothing is overwritten; the person sees both versions and chooses.

## The precondition: If-Match

Reading an entry returns its draft with an `ETag` that names the draft's revision. A save sends it back in `If-Match`:

```http
PUT /api/v1/projects/0192a7…/environments/production/collections/articles/entries/0192c1…
If-Match: "0192f3c4-…"
Idempotency-Key: 5f1c2a9e-save-0007
Content-Type: application/json

{"title":"Release notes","body":{…}}
```

| The request | The answer |
|---|---|
| `If-Match` names the current draft | `200`, a new revision, and its `ETag` |
| `If-Match` names an older revision | `409`, `details.reason` `stale_revision`, and the current `ETag`; nothing is written |
| No `If-Match` | `428 Precondition Required` |
| A malformed `If-Match`, or `If-Match: *` | `400` |
| Content its content type refuses | `422` with the refusal's code and JSON Pointer in `details` |
| A URL name another entry of the collection has, as a draft or published | `409`, `details.reason` `slug_taken`, naming that entry <Status done="3.6" /> |

The `ETag` names the revision, not a hash of its content, because the rule compares revisions: two revisions can hold identical content, and a save built on the older one is still stale.

Inside the transaction the save locks the entry row (`SELECT … FOR UPDATE`), so concurrent saves queue, and moves the pointer with `UPDATE entries SET draft_revision_id = new … WHERE draft_revision_id = expected`: the rule is the database's conditional update, not a check that can race. On the step 3.6 branch a save also takes its collection's content-type lock, shared, before it reads the content type, so a content-type change can't slip in between the check and the commit ([Content types and URL names](/concepts/content-types/)). <Status building="3.6" />

## Retries: Idempotency-Key

A save whose answer was lost can't know whether it landed. Sending it again as a fresh request would get a `409` from its own earlier save. So creates and saves both require an `Idempotency-Key` (16 to 128 characters of `A-Z a-z 0-9 - _`):

- the same key with the same request returns the recorded result, with `Idempotent-Replayed: true`;
- the same key with a different request is `409` with `details.reason` `idempotency_key_reused`;
- only successful answers are recorded, so a failed attempt rolls back and can be tried again.

The key is claimed first, inside the save's transaction, so a duplicate sent while the first is still running waits for it and then replays its answer. Keys are scoped to the person, the project, the environment and the operation.

## What the Studio does with this

The Studio keeps each edit on the device first, saves one request at a time a second after typing stops, and keeps the same key while the base revision and the content are unchanged. A `409` opens the conflict screen with both versions: **Keep my version** saves yours on top of theirs (theirs stays in the history), **Use their version** discards your unsaved changes. The device copy also keeps a request that was in flight, so reopening the page resends exactly that request: a save that committed moments before the tab closed is replayed, not stored twice. Kept edits expire after 30 days and are removed as soon as the server has them. The [editor guide](/editors/writing/) describes it from the editor's side.

## How it is tested

- **F01** and **F02** kill the real server immediately before and immediately after a save commits (`crates/colophon-server/tests/faults.rs`, built with compiled-in fault points): nothing unacknowledged is stored, nothing acknowledged is lost, and the retry is answered from the record.
- **F04**: two editors save on the same base revision. One gets the `409`; neither loses an accepted edit. It runs in two real browsers, in the database and in the simulator.
- The [simulator](/concepts/simulator/) models the Studio's saver too. That is how it found a real bug: after a reload, the Studio resent a kept edit with a new key, which could store it twice. The device copy now keeps the request in flight (D-038, D-039).
- `perf/k6/entries.js` is the load scenario for creates, saves and lists.

## Design

[Module 04, api](/design/modules/04-api/) (preconditions, idempotency) and [module 11, studio](/design/modules/11-studio/) (the save model); decisions D-035, D-036, D-038 and D-039 in the [decisions](/design/decisions/). On GitHub: [docs/modules/04-api.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/docs/modules/04-api.md).
