# `colophon/rich-text` version 1

The stored format of every rich-text field. It is [Tiptap](https://tiptap.dev) (ProseMirror) JSON restricted to an
allowlist, wrapped in a versioned envelope. This document is normative: the Studio's editor (`packages/editor`), the Rust
validator (step 0.5, `colophon-domain`) and the renderer all implement it, and the shared fixtures in
`fixtures/rich-text/v1/` prove they agree.

Rules that apply everywhere:

- **Nothing executable is ever stored.** No HTML, no scripts, no styles, no event handlers, no embeds from other sites.
- **Unknown fields fail validation.** Data is never dropped silently (module 01).
- **Rust is the authority.** The editor can't produce what the format forbids, but the server checks every save anyway.

## Envelope

```json
{ "schema": "colophon/rich-text", "version": 1, "doc": { "type": "doc", "content": [ … ] } }
```

| Field | Rule |
|---|---|
| `schema` | exactly `"colophon/rich-text"` |
| `version` | exactly `1` |
| `doc` | a `doc` node |

## Nodes

`block` means any of: `paragraph`, `heading`, `bulletList`, `orderedList`, `blockquote`, `codeBlock`, `horizontalRule`,
`image`, `callout`. `inline` means `text` or `hardBreak`.

| Node | Content | Attributes |
|---|---|---|
| `doc` | one or more `block` | none |
| `paragraph` | zero or more `inline` | none |
| `heading` | zero or more `inline` | `level`: 2, 3 or 4 (level 1 is the entry's title) |
| `bulletList` | one or more `listItem` | none |
| `orderedList` | one or more `listItem` | `start` (optional): integer 2 to 100000; absent means 1 |
| `listItem` | a `paragraph`, then zero or more `block` | none |
| `blockquote` | one or more `block` | none |
| `codeBlock` | zero or more `text`, without marks | `language` (optional): matches `^[a-z0-9][a-z0-9+#.-]{0,19}$` |
| `horizontalRule` | none | none |
| `image` | none | `assetId`: a lowercase UUID naming an asset in Colophon's media library; `alt`: 1 to 300 characters, not only whitespace |
| `callout` | one or more `paragraph` | `tone`: `note`, `tip` or `warning` |
| `text` | `text`: a non-empty string; `marks` (optional) | none |
| `hardBreak` | none | none |

`image` never holds a URL. Images come only from Colophon's media library, through the upload pipeline (module 07);
`alt` is required because every image needs a text alternative.

## Marks

Marks apply to `text` nodes in any node except `codeBlock`.

| Mark | Attributes | Rule |
|---|---|---|
| `bold`, `italic`, `strike` | none | |
| `code` | none | can't be combined with any other mark |
| `link` | `href` | `https:`, `http:` or `mailto:` URLs (as a browser's `URL` parser reads them); paths starting with exactly one `/`; fragments starting with `#`. At most 2048 characters. No control characters, whitespace or backslashes anywhere: browsers strip tabs and newlines and read `\` as `/`, so `/<tab>/evil.example` would lead to another site. |

A mark appears at most once on a node.

## Limits

| Limit | Value |
|---|---|
| Depth | at most 16; `doc` is depth 1, and each level of `content` adds 1 (so a `text` in a `paragraph` is depth 3) |
| Size | at most 262,144 bytes (256 KiB) of canonical JSON |
| Nodes | at most 10,000, counting every node including `text` |

## Canonical form

The editor always produces this form, and stored documents are always in it, so that a document loaded into the editor
and saved again is byte-for-byte identical.

- Keys appear in this order: `type`, `attrs`, `text`, `marks`, `content`.
- `attrs`, `marks` and `content` are left out when empty.
- Optional attributes are left out when they have their default (`orderedList.start` of 1, `codeBlock` without a language).
- Marks appear in this order: `link`, `bold`, `code`, `italic`, `strike` (their rank in the editor schema; `code` never
  appears with another mark).
- Serialized without whitespace.

Two editor details are normalised on the way out, because they carry no meaning: marks on a `hardBreak` are removed, and
the editor's internal attributes (`orderedList.type`, a link's `target`, `rel`, `class` and `title`) are never stored.

## Error codes

Every rejection has a code and a [JSON Pointer](https://www.rfc-editor.org/rfc/rfc6901) to the offending value.

| Code | Meaning | Pointer |
|---|---|---|
| `unsupported_schema` | `schema` isn't `colophon/rich-text` | `/schema` |
| `unsupported_version` | `version` isn't `1` | `/version` |
| `wrong_type` | a value has the wrong JSON type | that value |
| `missing_field` | a required field is absent | where it should be |
| `unknown_field` | a field the format doesn't define | that field |
| `unknown_node` | a node type outside the allowlist | its `type` |
| `invalid_content` | a node not allowed in that position, or required content missing | the node, or the parent's `content` |
| `unknown_mark` | a mark type outside the allowlist | its `type` |
| `invalid_marks` | marks on code-block text, `code` combined with another mark, or a repeated mark | the mark |
| `invalid_attribute` | an attribute value outside its rule | that attribute |
| `unsafe_link` | a link `href` that isn't an allowed scheme, path or fragment | the `href` |
| `empty_text` | a `text` node with an empty string | its `text` |
| `too_deep` | nesting beyond depth 16 | the first node past the limit |
| `too_large` | over 256 KiB of canonical JSON | the document root (empty pointer) |
| `too_many_nodes` | over 10,000 nodes | the document root (empty pointer) |

When a document has several problems, the validator reports the first one, in this order: the envelope (`schema`,
`version`, unknown fields, `doc`), then the tree depth-first. Within a node: the node count, its depth, its `type`,
whether it may appear in that position, its unknown fields (in sorted order), then `attrs`, `text`, `marks` and
`content`. Within `attrs`: unknown attributes first, then the defined ones in the order of the table above.

Character counts (alt text, links) are Unicode code points.

The validator also writes the canonical form. A valid document that differs from it only in key order, mark order, whitespace,
or an empty `attrs`, `marks` or `content` is accepted and stored canonically: none of those carry meaning. Anything else
is refused, never rewritten.
