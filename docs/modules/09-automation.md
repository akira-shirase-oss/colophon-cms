# 09 · automation (`crates/colophon-automation`)

> Safe, bounded access for scripts and agents. They can do permitted work at scale, and they can never approve their
> own changes or bypass review.

| | |
|---|---|
| Milestones | P5 |
| Proposed owner | Claude |
| Depends on | domain, auth (machine credentials, policy), api services, db (budget reservations), worker (bulk and AI pools), an MCP implementation (O-013) |
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
- The CLI's JSON contract is implemented first; the MCP adapter wraps the same services afterwards.

### MCP adapter (spec §11; O-013)
- Remote access uses the pinned MCP 2025-11-25 authorization requirements, with audience validation.
- Local stdio uses scoped credentials supplied by the operator.
- Neither is a privileged alternate path: both call the identical services.

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
4. The MCP adapter.
5. The optional AI assistant, last and off by default.

## Open decisions

- **O-013:** the MCP implementation (official Rust SDK or a thin JSON-RPC layer).
- Whether the AI assistant ships in v0.1 at all, or moves to "Later".
- The default budget values per project.
