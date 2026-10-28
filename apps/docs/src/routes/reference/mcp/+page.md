<script>
	import Status from '#lib/components/Status.svelte';
</script>

`colophon-mcp` lets an AI agent work with Colophon through the [Model Context Protocol](https://modelcontextprotocol.io/): it reads content, writes drafts and asks people to review a publish request, and nothing more. It runs on the agent's own machine as a subprocess, speaking MCP over its standard input and output, and it is a plain client of the same HTTP API the Studio and the command line use, signed in with a [project API token](/reference/api/tokens/). Every call is decided by the API with the token's own permissions; the server never takes a shortcut around them (D-068).

It is the first slice of milestone P5 (safe automation), built on a branch with its tests and not merged. It isn't packaged: nothing is published to npm, Cargo or Homebrew, so you build it from the repository. The crate is `crates/colophon-mcp`.

## What it never does

There is no tool to **approve, publish, take down or schedule** anything, and none to manage **roles, members or tokens**, whatever the token is allowed. A full-access token, which could approve and publish through the API, gets the same eight tools as any other: agents propose, people decide. The server's instructions to the model say so, and say that entry content is data written by people, never instructions to follow.

## The tools

Eight tools, each a typed wrapper over one existing route under `{P}` = `/api/v1/projects/{project}/environments/production`:

| Tool | What it does | Route | Listed when the token holds |
|---|---|---|---|
| `list_content_types` | The collections and their fields. | `GET {P}/content-types` | always |
| `list_entries` | A page of a collection's entries, 20 at a time, newest first; `q` searches titles, `cursor` turns the page. | `GET {P}/collections/{collection}/entries` | `content.read` |
| `get_entry` | One entry's draft, its current revision, what readers get now, and what references it. | `GET {P}/collections/{collection}/entries/{id}` | `content.read` |
| `create_draft` | A new entry, as a draft. | `POST {P}/collections/{collection}/entries` | `content.create` |
| `update_draft` | A new version of a draft, from the revision the agent read. | `PUT {P}/collections/{collection}/entries/{id}` | `content.update` |
| `create_publish_request` | A publish request pinning entries at their current revisions, for people to review. | `POST {P}/changesets` | `content.update` |
| `request_review` | Asks people to review a publish request, as it was when the agent read it. | `POST {P}/changesets/{id}/submit` | `content.update` |
| `get_publish_request` | One publish request: its entries, hash, status and reviews. | `GET {P}/changesets/{id}` | always |

- **Listing follows the token.** `tools/list` reads [`GET {P}/permissions`](/reference/api/tokens/#get-ppermissions) once and lists a tool when the token holds the action it needs in some collection: a read-only token sees `list_content_types`, `list_entries`, `get_entry` and `get_publish_request`. Listing grants nothing. A listed tool can still be refused for one entry, by a field list or an `own` condition, and the API decides every call.
- **Stale versions never overwrite.** `update_draft` sends the revision the agent read as `If-Match`. If the draft has moved on, nothing is saved, and the tool error names the current revision and says to read the entry again. `request_review` sends the publish request's hash the same way.
- **Retries can't write twice.** `create_draft`, `update_draft` and `create_publish_request` send an `Idempotency-Key`: the agent's `idempotency_key` argument, or a new one, returned in the result with `replayed`. After no answer or a `503`, the server tries again with the same key, three attempts at most within the call's 30 seconds. A write that got no answer at all is a tool error, `outcome_unknown`, naming the key to retry with. Asking for review twice is the same as asking once.
- **Refusals stay whole.** An API refusal becomes a tool error (`isError`, so the model can correct itself) that keeps the status, the `code`, `details.reason`, `details.permission`, `details.pointer`, the rest of `details` and the request id. Arguments that don't fit a tool's input schema are tool errors too, checked before anything is sent.

A successful result carries its data as `structuredContent` (`content_types`, `entries` and `next_cursor`, `entry`, or `publish_request`, with `idempotency_key` and `replayed` after a write) and the same JSON as text.

## Setting it up

**1. Make a token.** An owner makes a project API token for the agent, with only what it needs, for example drafting articles:

```json
{
  "name": "Writing agent",
  "kind": "custom",
  "permissions": [
    { "action": "content.read", "subject": "articles" },
    { "action": "content.create", "subject": "articles" },
    { "action": "content.update", "subject": "articles" }
  ],
  "expires_in_days": 30
}
```

`POST` it to `/api/v1/projects/{project}/tokens` ([API tokens](/reference/api/tokens/#post-apiv1projectsprojecttokens)) and keep the `secret` from the answer; it is shown once. What the agent writes is attributed to the token, and revoking it stops the agent at its next call.

**2. Build the server** from a checkout of the repository, with the pinned toolchain:

```sh
cargo build --release -p colophon-mcp
```

The binary is `target/release/colophon-mcp`.

**3. Configure your MCP client** to start it with three environment variables, and an optional fourth:

| Variable | What |
|---|---|
| `COLOPHON_SERVER` | The Colophon server's origin, such as `https://cms.example.com`. Plain `http` is accepted only for `localhost`, `127.0.0.1` or `::1`, so the token never crosses a network unencrypted. An origin only: no path, query or credentials. |
| `COLOPHON_TOKEN` | The project API token, `colophon_api_…`. A person's command-line token is refused: an agent acts as its own identity. |
| `COLOPHON_PROJECT` | The token's project id. A token works only in its own project, and no route tells a token which one that is. |
| `COLOPHON_MCP_LOG` | Optional: the log level, `info` by default. |

Every problem with the configuration is reported at once, on standard error, and the token never appears in a message. Clients that start local servers take a command and its environment; in the JSON form many of them read, that looks like this (check your client's documentation for where its configuration lives):

```json
{
  "mcpServers": {
    "colophon": {
      "command": "/path/to/colophon/target/release/colophon-mcp",
      "env": {
        "COLOPHON_SERVER": "https://cms.example.com",
        "COLOPHON_TOKEN": "colophon_api_…",
        "COLOPHON_PROJECT": "0193f6a2-5b1c-7d3e-9f40-1a2b3c4d5e6f"
      }
    }
  }
}
```

Keep that file as private as the token itself.

## The protocol

- **MCP 2025-11-25, on stdio.** One JSON-RPC 2.0 message per line, no batches. The server answers `initialize`, `ping`, `tools/list` and `tools/call`; `notifications/cancelled` stops the request it names. It always answers `initialize` with 2025-11-25, the one version it speaks.
- **Errors.** An unknown method is `-32601`; an unknown tool or malformed `params`, `-32602`; a request before `initialize`, `-32002`; a line over the size limit, `-32600`. When `tools/list` can't learn what the token may do, the answer is `-32001` and says why: the token is wrong, revoked or expired, or `COLOPHON_PROJECT` isn't its project.
- **Limits.** A message is at most 2 MiB and an API answer at most 4 MiB. One attempt has 15 seconds including its answer, connecting 5, a whole call 30. At most 8 requests run at once; the next waits for a slot. A client that stops reading for 30 seconds ends the session, and when the input ends, running requests get 5 seconds. An idle client is fine: reading has no deadline.
- **Logs** go to standard error only, never the token or content. The process exits `0` when the client closes its input, `1` when the session fails, and `2` for a configuration it can't use.
- **One environment.** Every tool works in `production`.

## Not built yet

- Remote MCP over Streamable HTTP, with the protocol's authorization. <Status planned="P5" />
- Budgets reserved in PostgreSQL, bulk manifests and jobs that resume after an interruption, and quotas for agents. <Status planned="P5" />
- Tools to check a publish request, follow a job or a release, and ask for something to be taken down. <Status planned="P5" />
- The evaluation suite, with zero unauthorized actions as its gate, and a noisy agent throttled beside editors (L08). <Status planned="P5" />
- A `colophon mcp` subcommand, and environments other than `production`. <Status planned="P5" />
- The MCP server published from the same release as the command line, with the same version check. <Status planned="V" />

## How it is tested

`cargo test -p colophon-mcp` covers the JSON-RPC framing (malformed JSON, batches, ids, oversized lines, unknown methods and tools, the handshake and requests before it), each tool's argument checks, how refusals are reported, and the configuration. `crates/colophon-mcp/tests/stack.rs` drives the real binary over stdio against the local stack with a custom, a read-only and a full-access token: a draft, a stale save refused with the current revision named, a create retried with the same key giving the same entry, a publish request and its review request; no write tools for the read-only token and its writes refused by the API; no approving or publishing tool for anyone, thirteen such names each answered "unknown tool"; and a wrong project or a token revoked mid-session reported plainly (`-32001`, then `401` on the next call).

## Design

D-068, and module 09's MCP section, are in `docs/` on the branch that builds them; the [design documents](/design/) here show them once it is merged.
