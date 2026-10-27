Every rich-text field is stored as `colophon/rich-text` version 1: [Tiptap](https://tiptap.dev) (ProseMirror) JSON restricted to an allowlist, in a versioned envelope. Never HTML. The Studio's editor, the Rust validator and the renderer all implement it, and shared fixtures (`fixtures/rich-text/v1/`) prove they agree. This page summarises it; the normative text is the [format document](/design/formats/rich-text-v1/).

Three rules apply everywhere:

- **Nothing executable is ever stored**: no HTML, scripts, styles, event handlers or embeds.
- **Unknown fields fail validation.** Data is never dropped silently.
- **Rust is the authority.** The editor can't produce what the format forbids, but the server checks every save anyway.

## The envelope

```json
{
  "schema": "colophon/rich-text",
  "version": 1,
  "doc": {
    "type": "doc",
    "content": [
      { "type": "heading", "attrs": { "level": 2 }, "content": [{ "type": "text", "text": "What changed" }] },
      {
        "type": "paragraph",
        "content": [
          { "type": "text", "text": "See the " },
          { "type": "text", "text": "changelog", "marks": [{ "type": "link", "attrs": { "href": "/changelog" } }] },
          { "type": "text", "text": "." }
        ]
      },
      { "type": "callout", "attrs": { "tone": "tip" }, "content": [{ "type": "paragraph", "content": [{ "type": "text", "text": "Saved drafts are never lost." }] }] }
    ]
  }
}
```

## Nodes and marks

| Node | Holds | Attributes |
|---|---|---|
| `paragraph` | text and line breaks | |
| `heading` | text | `level`: 2, 3 or 4 (level 1 is the entry's title) |
| `bulletList`, `orderedList` | list items | `orderedList.start`: 2 to 100000, optional |
| `listItem` | a paragraph, then blocks | |
| `blockquote` | blocks | |
| `codeBlock` | plain text, no marks | `language`, optional |
| `horizontalRule` | nothing | |
| `image` | nothing | `assetId` (from the media library, never a URL) and required `alt` text |
| `callout` | paragraphs | `tone`: `note`, `tip` or `warning` |

Marks on text: `bold`, `italic`, `strike`, `code` (which combines with no other mark) and `link`. A link's `href` is `https:`, `http:` or `mailto:`, a path starting with exactly one `/`, or a `#fragment`, at most 2,048 characters, with no whitespace, control characters or backslashes anywhere.

Images can be stored today, but there is no media library to upload them to until milestone P4, so the Studio shows them as placeholders.

## Limits

At most 16 levels deep, 256 KiB of canonical JSON, and 10,000 nodes.

## The canonical form

Stored documents are always canonical, so a document loaded into the editor and saved again is byte for byte identical, and its SHA-256 is stable: keys in the order `type`, `attrs`, `text`, `marks`, `content`; empty `attrs`, `marks` and `content` left out; optional attributes left out at their defaults; marks in the order `link`, `bold`, `code`, `italic`, `strike`; no whitespace. The server accepts a valid document that differs only in those ways and stores it canonically. Anything else is refused, never rewritten. The editor itself refuses to open a document it can't give back byte for byte.

`POST /api/v1/rich-text/validate` returns a document's canonical form, or the refusal, without storing anything.

## Error codes

A refusal is a `422` whose `details` name the code and a JSON Pointer to the offending value. When a document has several problems, the first one is reported, in a fixed order.

| Code | Meaning |
|---|---|
| `unsupported_schema` | `schema` isn't `colophon/rich-text`. |
| `unsupported_version` | `version` isn't `1`. |
| `wrong_type` | A value has the wrong JSON type. |
| `missing_field` | A required field is absent. |
| `unknown_field` | A field the format doesn't define. |
| `unknown_node` | A node type outside the allowlist. |
| `invalid_content` | A node not allowed in that position, or required content missing. |
| `unknown_mark` | A mark type outside the allowlist. |
| `invalid_marks` | Marks on code-block text, `code` with another mark, or a repeated mark. |
| `invalid_attribute` | An attribute value outside its rule. |
| `unsafe_link` | A link that isn't an allowed scheme, path or fragment. |
| `empty_text` | A text node with an empty string. |
| `too_deep` | Nesting beyond depth 16. |
| `too_large` | Over 256 KiB of canonical JSON. |
| `too_many_nodes` | Over 10,000 nodes. |

## Rendering it

`packages/content-renderer` turns a document into HTML: it builds every tag itself and escapes every piece of text, uses no inline styles or scripts (so it works under a strict Content Security Policy), and throws on anything outside the format rather than guessing. The Studio's preview and the reference website both use it, so they can't drift. Its output for every valid fixture is reviewed and kept in `fixtures/rich-text/v1/rendered/`.

Tables, embeds and footnotes would be a version 2 with its own fixtures; stored version 1 documents stay valid.
