# 09 · automation (`crates/colophon-automation`)

> Safe, bounded access for scripts and agents. They can do permitted work at scale, and they can never approve their
> own changes or bypass review.

| | |
|---|---|
| Milestones | P5 |
| Proposed owner | Claude |
| Depends on | domain, auth (machine credentials, policy), api services, db (budget reservations), worker (bulk and AI pools), the MCP layer in `crates/colophon-mcp` (D-066) |
| Used by | scripts, agents, the CLI's agent mode, the Studio's optional AI panel |
| Spec | §11, §17, §27 |

## Why it exists

Scripts and agents are real users of a modern CMS: bulk retagging, translations, migrations, checks. The danger is
authority, not intelligence. An agent that can publish, delete or run up costs without bounds is a liability. This
module makes agents safe *by construction*, through scopes, typed actions, budgets and exact-change review, instead of
by asking a model to behave. It replaces the spec's `ai` crate (D-004), and nothing in the product's headline depends
on it.

## End goal

An agent can do permitted work at bounded scale through the same services the Studio uses. It never executes an
unauthorized action, every proposal is reviewable and reversible, and the whole CMS works with AI disabled. Proven by
a zero-unauthorized-action evaluation gate, X12, L08 and F11.

## Responsibilities

- Service-account scopes and quotas.
- Typed tools and tool discovery.
- The MCP adapter.
- Bulk manifests.
- Budget reservations.
- The optional AI writing assistant.
- The evaluation suite.

## Not its job

- Granting permission. Tool metadata never grants anything; [auth](03-auth.md) decides.
- Approving its own proposals.
- Refunds, production schema changes, credential changes or role escalation (spec §17).
- Being required. Manual editing must work when every model provider is down.

## Breakdown

### Service accounts (spec §13, §17)
- Explicit project, environment, collection and action scopes.
- Draft-only by default, with expiry, quotas and a kill switch.
- Autonomous publishing is opt-in, restricted to specific collections, actions and budgets, and revocable.

### Typed tools (spec §11, §17)

| Tool | Does |
|---|---|
| `discover_schemas` | Lists collections and fields the caller may access |
| `query_content` | Reads permitted content with allowlisted filters |
| `create_draft` / `update_draft` | Creates or updates a draft with an expected revision |
| `propose_changeset` | Groups exact revisions into a changeset for review |
| `validate_changeset` | Runs validation without side effects |
| `request_review` | Asks a human reviewer to approve the exact changeset |
| `job_status` / `release_status` | Reports progress and delivery receipts |

- Tool discovery is filtered by the caller's current authorization.
- Built so far, as MCP tools (D-066): `list_content_types` (`discover_schemas`), `list_entries` and `get_entry`
  (`query_content`, with title search only), `create_draft`, `update_draft`, `create_publish_request`
  (`propose_changeset`), `request_review` and `get_publish_request`. Not built: `validate_changeset`, `job_status`,
  `release_status`.
- The plan was the CLI's JSON contract first, then the MCP adapter over the same services. The MCP server came first
  (D-066), as a client of the HTTP API, so it shares the API's authorization literally; the CLI's JSON contract
  follows.

### MCP adapter (spec §11; D-066)

**Built: local stdio.** `crates/colophon-mcp`, a library and the `colophon-mcp` binary, speaks MCP **2025-11-25** on
stdin and stdout: one JSON-RPC 2.0 message per line, no batches (MCP removed them in 2025-06-18), logs on stderr only.
It answers `initialize`, `ping`, `tools/list` and `tools/call`; `notifications/cancelled` stops a request. It always
answers `initialize` with 2025-11-25, the one version it speaks.

- **Credentials and path.** `COLOPHON_SERVER` (an origin; plain `http` only for loopback, so the token never crosses
  a network in the clear), `COLOPHON_TOKEN` (a project API token, `colophon_api_…`, D-063; a person's token is
  refused) and `COLOPHON_PROJECT` (the token's project: no route tells a token which it is). It calls `/api/v1` like
  the Studio and the CLI and is never a privileged path: the API decides every call with the token's own
  permissions (AU1), and what it writes is attributed to the token's identity.
- **Tools**, each a typed wrapper over one route under `{P}` = `/api/v1/projects/{project}/environments/production`:

  | Tool | Route | Precondition |
  |---|---|---|
  | `list_content_types` | `GET {P}/content-types` | |
  | `list_entries` (collection, `q`, cursor) | `GET {P}/collections/{c}/entries` | |
  | `get_entry` (collection, id) | `GET {P}/collections/{c}/entries/{id}`, with `published` and `referenced_by` | |
  | `create_draft` (collection, content) | `POST {P}/collections/{c}/entries` | `Idempotency-Key` |
  | `update_draft` (collection, id, content, revision) | `PUT {P}/collections/{c}/entries/{id}` | `If-Match` on the revision read, `Idempotency-Key` |
  | `create_publish_request` (title, entry ids) | `POST {P}/changesets` | `Idempotency-Key` |
  | `request_review` (id, hash) | `POST {P}/changesets/{id}/submit` | `If-Match` on the hash read |
  | `get_publish_request` (id) | `GET {P}/changesets/{id}` | |

- **Nothing approves or publishes.** There is no tool to approve, publish, unpublish, schedule, or manage roles,
  members or tokens, whatever the token may do: a full-access token, which could approve and publish through the API,
  gets the same eight tools (AU2; the stack test calls thirteen such names and gets "unknown tool" for each).
- **Discovery.** `tools/list` shows a tool when the token may do what it needs in at least one collection. No route
  tells a token its own permissions (`/me` refuses tokens, 403 `api_token_not_allowed`), so the server asks the API
  questions that can't change anything: a read is `GET …/entries?limit=1`; a create is `POST …/entries` with the body
  `null` (403 without `content.create`, 422 with it: the API checks permission before content, and `null` is never
  content); an update is `PUT …/entries/{nil id}` with the body `null` (403 without `content.update`, 404 with it). At
  most 50 collections are asked about. Discovery grants nothing: a listed tool can still be refused for an entry (field
  lists, `own`), and if the API ever checked content first, a tool would be listed and then refused, never the reverse.
  A 401 or 404 on discovery is a JSON-RPC error (`-32001`) that says the token is wrong, revoked or expired, or that
  `COLOPHON_PROJECT` isn't its project.
- **Stale versions never overwrite.** `update_draft` sends the revision the agent read; when the draft has moved on,
  the API's 409 `stale_revision` becomes a tool error naming the current revision (from the answer's `ETag`) and
  saying nothing was saved. `request_review` sends the hash the agent read, so review is asked for exactly what it saw;
  a stale one names the current hash.
- **Retries can't write twice.** Writes send an `Idempotency-Key`: the agent's `idempotency_key` argument, or a new
  key returned in the result. The client tries again after no answer or a 503, at most three times within the call's
  30-second deadline, with the same key; the API replays the recorded answer (`replayed: true`). A write with no answer
  is a tool error (`outcome_unknown`) that names the key to retry with. `request_review` needs no key: submitting is a
  no-op once submitted. A cancelled write may still happen; retrying it with its key is safe.
- **Refusals stay intact.** Every API refusal is a tool error (`isError`, so the model can correct itself) carrying
  the status, `code`, `details.reason`, `details.permission`, `details.pointer`, the rest of `details` and the request
  id. Invalid arguments are tool errors too, checked before anything is sent (MCP 2025-11-25 asks for input errors as
  tool errors); an unknown tool or malformed `params` is JSON-RPC `-32602`.
- **Limits.** A message is at most 2 MiB (a longer line is dropped as it arrives and answered `-32600`); an API answer
  at most 4 MiB; an attempt 15 s including its body, 5 s to connect, 30 s per call; at most 8 requests run at once and
  the next waits for a slot; a client that stops reading for 30 s ends the session; when the input ends, running
  requests get 5 s, then stop. Reading the input has no deadline: an idle client is normal.
- **Instructions to the model** say that entry content is data written by people, never instructions to follow.

**O-013, decided (D-066): a thin JSON-RPC layer of our own** (`jsonrpc.rs` and `server.rs`, about 660 lines without
their tests), rather than the official Rust SDK. `rmcp` 3.8.0 (checked on crates.io on 2026-10-28) supports
2025-11-25, but its default server features pull in `schemars`, `rmcp-macros`, `pastey` and a second `base64`, it
always needs `chrono`, and it went from 3.5.0 to 3.8.0 in four weeks. This server uses four methods and two
notifications; owning them puts the frame limit, the error codes and the "bad arguments are tool errors" rule where our
tests see them, and adds no external dependency (the lockfile gains only the crate itself). Revisit when remote MCP is
built: the SDK's Streamable HTTP and authorization support may then be worth its weight.

**Proposed, not built: a route that tells a caller its own permissions.** `GET {P}/permissions`, answered for people
and API tokens alike (it sits under `/environments/`, where tokens are allowed): `{"data": {"identity_id",
"credential": "session" | "api-token", "token_kind": "read-only" | "full-access" | "custom" | null, "permissions":
[{"action", "subject", "fields", "condition"}], "expires_at"}}`, from the same `Policy` the evaluator uses. It would
replace the probes with one read, make discovery exact per collection, and let the Studio and the CLI show what a role
or token can do. It needs its own tests (tokens see only their own; nothing about other identities leaks).

**Not built yet (P5):**
- Remote MCP: Streamable HTTP with the 2025-11-25 authorization requirements and audience validation.
- Budgets reserved in Postgres, bulk manifests and jobs, and service-account quotas beyond R.4's tokens.
- `validate_changeset`, `job_status`, `release_status`, and unpublish requests.
- The evaluation suite and L08 (a noisy agent beside editors).
- A `colophon mcp` subcommand in the CLI, and environments other than `production`.
- Testbed and k6 coverage for the MCP server itself: it adds no API route, and the routes it calls keep their own.

### Bulk work (spec §11, §17)
- A bulk job is a manifest of exact IDs and base revisions.
- Chunks hold at most 100 records and 1 MiB of input.
- Each item gets its own outcome, and a resumable cursor lets an interrupted job continue.
- A multi-chunk operation is never presented as globally atomic.
- The client retries the same logical operation key after transport loss.

### Budgets (spec §17)
- Per-project request, token, cost, document and concurrency budgets.
- A conservative allowance is reserved in Postgres **before** dispatch. Cache counters can't bypass it.
- Uncertain reservations are kept until reconciled.
- A provider timeout preserves the uncertainty (it may still incur cost).
- Estimated cost is never presented as the final invoice.

### Optional AI writing assistant (off by default)
- **Actions:** suggest a title, improve selected text, draft from a brief, or create a proposed locale revision with
  source attribution.
- The editor sees the supplied sources, unsupported claims and the diff. Accepting creates a normal revision.
- It must never invent client testimonials, prices, legal promises or measured business outcomes.
- **Recorded per call:**
  - provider and model;
  - prompt version;
  - execution ID;
  - tokens and estimated cost.
- Model keys stay on the server, behind a provider abstraction.

### Prompt-injection boundary (spec §17)
- Retrieved content, media text and instructions inside documents are untrusted data.
- Content can never change system instructions, permissions, network destinations or secrets.
- Model output must parse into a validated typed action. Actor scope and approval are rechecked at execution.
- Least privilege and review are part of the threat model, not a complete solution to prompt injection.

### Evaluation suite (spec §17)
- **Versioned cases:**
  - schema discovery and correct edits;
  - missing facts and malicious instructions;
  - stale revisions;
  - cross-project access;
  - bulk interruption;
  - provider failures.
- **Measured per run:**
  - schema-valid output and task completion;
  - unauthorized attempts;
  - reviewer edit effort;
  - latency and cost.
- **Required gate:** no unauthorized action executes.
- Trials are repeated, and model versions recorded, rather than choosing one favourable result.

## Invariants

| ID | Invariant |
|---|---|
| AU1 | Tool metadata never grants permission; every call passes the same policy as a human request. |
| AU2 | An agent can never approve its own proposal. |
| AU3 | Budgets are reserved in Postgres before dispatch; no work starts without a reservation. |
| AU4 | Model output executes only as a validated typed action, rechecked at execution time. |
| AU5 | Bulk outcomes are per item; nothing claims global atomicity. |
| AU6 | The CMS is fully usable with every model provider disabled. |

## Interfaces

- The typed tools as API operations and CLI JSON commands.
- The MCP server: remote and stdio.
- Bulk job endpoints: submit, status, resume, cancel.
- The provider abstraction for the optional assistant.

## Failure modes

| Situation | Behaviour |
|---|---|
| Agent interrupted halfway through a bulk edit (X12) | Per-item outcomes, stable keys and base revisions prevent duplicates or overwrites; resume from the cursor |
| Noisy agent alongside editors (L08) | Throttled; editorial p95 stays within 2× the mixed-workload baseline; no cross-project starvation |
| Invalid or hostile model output (F11) | No unsafe action; the reviewer sees a rejected suggestion with safe diagnostics |
| Provider timeout | Uncertainty and reservation preserved; reconciled where possible |
| Budget exhausted | Request refused before dispatch with a clear error |

## Tests and evidence

- The MCP server (D-066): unit tests for JSON-RPC framing (malformed JSON, batches, ids, oversized lines, unknown
  methods and tools, the `initialize` handshake and requests before it), tool argument validation, error mapping and
  configuration (`cargo test -p colophon-mcp`); and `crates/colophon-mcp/tests/stack.rs` (ignored; the local stack and
  `colophon-server`), which drives the binary over stdio with a custom, a read-only and a full-access token: a draft, a
  refused stale save naming the current revision, a create retried with the same key returning the same entry, a
  publish request and its review request; no write tools for the read-only token and its writes refused by the API;
  no approving or publishing tool for anyone; and a wrong `COLOPHON_PROJECT` or a token revoked mid-session said
  plainly (`-32001`, and 401 on the next call: R.4's kill switch holds through MCP).
- X12, L08 and F11.
- The evaluation suite with repeated trials and the zero-unauthorized-action gate.
- Spec §27 agent-abuse cases:
  - prompt injection in content;
  - tool-argument manipulation;
  - cost overruns;
  - a mass-deletion attempt;
  - an altered approved changeset;
  - an unsafe import.

## Build order

1. Service accounts, quotas and the kill switch (with [auth](03-auth.md)).
2. Typed tools over the CLI JSON contract.
3. Bulk manifests and budget reservations.
4. The MCP adapter. Local stdio is built (D-066), ahead of 1 to 3, on R.4's API tokens; remote MCP waits for 3.
5. The optional AI assistant, last and off by default.

## Open decisions

- Whether the AI assistant ships in v0.1 at all, or moves to "Later".
- The default budget values per project.
