# Design tokens (D-210)

`design/tokens.css` is the one source of Colophon's colours, radius, shadows, spacing and font stacks, for the Studio,
the website and the docs. Everything else copies it, byte for byte; nothing edits a copy.

| File | What it is | Checked by |
|---|---|---|
| `design/tokens.css` | the source: `:root` (light) and `.dark` | `node scripts/design-contrast.mjs` (every pair meets its bar) |
| `docs/design/contrast.md` | the contrast table, generated from the source | `node scripts/design-contrast.mjs --check docs/design/contrast.md` |
| `packages/ui/src/lib/tokens.css` | the Studio's copy | `node scripts/design-contrast.mjs --check-copy packages/ui/src/lib/tokens.css` |
| `packages/ui/src/lib/fonts/geist-pixel-latin.woff2` | the landing's Geist Pixel subset, with `OFL-GeistPixel.txt` | its sha256, recorded in the source's header |

`scripts/frontend.sh` runs all three checks, so the quick check and CI fail on a pair below its bar, a stale table, or
a drifted copy.

## Changing a token

1. Edit `design/tokens.css` only.
2. `node scripts/design-contrast.mjs --write docs/design/contrast.md` and read what moved; a failing pair is a design
   decision to make, not a number to nudge silently.
3. `cp design/tokens.css packages/ui/src/lib/tokens.css`.
4. Rebuild the Studio and look at the screens in both themes (`/studio/design` shows every token and component).
5. The website and the docs pick it up from a release (below).

## The website's and the docs' copies

They live in other repositories, so they can't import this file. Each keeps a byte copy (for example
`src/styles/tokens.css`) and checks it in its own CI against this repository at a **pinned ref**, a release tag, so a
token change here never breaks their build by surprise; moving the pin is the deliberate step that adopts it:

```sh
# In the website's or the docs' CI. COLOPHON_REF is a tag, e.g. v0.1.0-alpha.1.
set -euo pipefail
COLOPHON_REF=v0.1.0-alpha.1
curl -fsSL "https://raw.githubusercontent.com/akira-shirase-oss/colophon-cms/$COLOPHON_REF/design/tokens.css" -o /tmp/colophon-tokens.css
cmp -s /tmp/colophon-tokens.css src/styles/tokens.css || {
  echo "src/styles/tokens.css differs from colophon-cms $COLOPHON_REF design/tokens.css: copy it again" >&2
  exit 1
}
# The font the tokens name: the same bytes as the source records.
echo "a50494ed7ca60a21f2ffb7fd93bc996aaa7b5a2f0246184543ad0dfd5f557ed5  public/fonts/geist-pixel-latin.woff2" | sha256sum -c -
```

A site that needs only part of the file (the website has no use for the status palette) still copies all of it:
unused custom properties cost nothing, and a partial copy can't be checked.
