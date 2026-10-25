# Rich-text fixtures

The contract between everything that touches articles: the format is
[docs/formats/rich-text-v1.md](../../docs/formats/rich-text-v1.md), and these files prove that the parts agree.

| Folder | What each file holds | Who checks it |
|---|---|---|
| `v1/valid/` | A document in canonical form | The editor reproduces it byte for byte (`packages/editor`); the Rust validator accepts it and writes the same bytes (`colophon-domain`); the HTTP endpoint returns it (`colophon-api`, the testbed) |
| `v1/invalid/` | A document, and the `reject` code and JSON Pointer it must be refused with | The Rust validator and the endpoint refuse it with exactly that code and pointer; the editor refuses it or keeps it intact; the renderer refuses it or renders it safely |
| `v1/rendered/` | The reviewed HTML for each valid document, with images on `https://assets.example/` | The renderer reproduces it (`packages/content-renderer`); the Studio's preview is the same DOM |

Adding a fixture: write it in canonical form (key order `type`, `attrs`, `text`, `marks`, `content`), run the editor,
validator and renderer tests, and review any new file in `rendered/` before committing it. A changed `rendered/` file
is a change to what readers see: give it the same review as code.
