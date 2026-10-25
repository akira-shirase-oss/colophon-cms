# 12 · content-renderer (`packages/content-renderer`)

> One renderer for the Studio preview and the public site, so what an editor previews is what readers get.

| | |
|---|---|
| Milestones | P0 (shared fixtures), P1–P3 (renderer used by the Studio and the blog) |
| Proposed owner | Claude |
| Depends on | Tiptap JSON produced by the Studio and validated by [domain](01-domain.md) |
| Used by | studio (preview), web/blog template, release validation (renderer version) |
| Spec | §05, §19, §26 |

## Why it exists

If the Studio preview and the public site render rich text differently, editors stop trusting the preview. And if a
renderer accepts raw HTML, stored XSS follows. A single, versioned renderer with shared fixtures fixes both.

## End goal

Validated Tiptap JSON renders to identical, safe output in the Studio preview and on the public site. This is proven by
fixtures shared with the Rust validator and by XSS payload tests.

## Responsibilities

- Rendering the allowed nodes, marks and embeds.
- Link sanitization.
- The renderer version number.
- The shared fixture set.

## Not its job

- Validation authority: Rust validates every mutation.
- Editing behaviour: the Tiptap editor lives in the Studio.

## Breakdown

- **Allowlist:** renders only allowed nodes, marks and embeds, and only links with allowed URL schemes.
- **No raw HTML:** content is never rendered as raw HTML, and executable markup is never produced.
- **Versioning:**
  - every release records the renderer version it was validated against;
  - a content block the selected renderer does not support blocks publication before it happens (spec §19).
- **Shared fixtures:** one JSON fixture set is used by the Rust validator and by this renderer. Valid fixtures must
  render; invalid ones must be rejected by Rust.
- **Security headers:** rendered output is compatible with a restrictive Content Security Policy (spec §26).

## Invariants

| ID | Invariant |
|---|---|
| R1 | Studio preview and public output are identical for the same document and renderer version. |
| R2 | No raw HTML or executable markup is ever emitted. |
| R3 | Unsupported blocks fail validation before publication, never at render time on the live site. |

## Interfaces

- `render(document, options) → output`, available both as Svelte components and as a string renderer.
- `RENDERER_VERSION`.

## Failure modes

| Situation | Behaviour |
|---|---|
| Unknown node in a stored document | Validation already rejected it; the renderer treats it as a bug and logs it once |
| Hostile link (`javascript:` and similar) | Not rendered as a link |

## Tests and evidence

- Snapshot tests over the shared fixtures.
- XSS payload fixtures: hostile links, stored markup attempts.
- A Rust/TypeScript agreement test: the same fixtures are accepted or rejected identically.

## Build order

1. **P0:** fixture set and basic nodes. Built in step 0.6: `packages/content-renderer` (`renderToHtml` and a `RichText`
   Svelte wrapper around it), reviewed expected output in `fixtures/rich-text/v1/rendered/`, and an allowlist check that
   parses every rendered page back into a DOM (D-023).
2. **P1–P3:** full v0.1 block set, used in the Studio and the blog.

## Open decisions

- None for the block set: it is `colophon/rich-text` v1 (D-021, [formats/rich-text-v1.md](../formats/rich-text-v1.md)).
