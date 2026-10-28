<script>
	import Status from '#lib/components/Status.svelte';
</script>

Colophon has one management API that everything uses, and one public API for websites. These pages describe them as the code answers today; when a page and the code disagree, the code is right and the page is a bug.

## HTTP

- [Management API conventions](/reference/api/): authentication, response shapes, `If-Match` and ETags, idempotency keys, pagination, limits and status codes.
- [Management API routes](/reference/api/routes/): every route the server answers, and who may call it.
- [Errors](/reference/api/errors/): the error envelope, its stable codes and the reasons in `details`.
- [Delivery API](/reference/delivery-api/): `/content/v1`, public and read-only, latest or pinned to a release.

## Tools and formats

- [Command line](/reference/cli/): the `colophon` command.
- [Rich-text format](/reference/rich-text/): `colophon/rich-text` v1, the stored JSON of every rich-text field.
- [Webhooks and website connections](/reference/webhooks/): webhooks, CDN purges and probes, and what each receives.

## Not built yet

- A generated OpenAPI document, so every API change shows in review (D-027), and the TypeScript SDK with generated types, with a helper for reads pinned to one release (module 13). <Status planned="D" />
- One version across the server, the CLI, the SDK and the MCP server, the server reporting the API versions it serves, and the CLI installable from npm (`@colophon-cms/cli`), Cargo and Homebrew without building Rust (D-062). <Status planned="V" />
- API tokens for scripts, with the same permissions as a role, and an audit log of role, membership and token changes. <Status planned="R.4" />
- Uploads and media, preview links, rollback. <Status planned="P4" />
- Scoped machine credentials, bulk jobs, an MCP adapter for agents, and the CLI's full JSON contract with import and export. <Status planned="P5" />
