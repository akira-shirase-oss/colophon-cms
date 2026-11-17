# @colophon-cms/rich-text

Render [Colophon CMS](https://github.com/akira-shirase-oss/colophon-cms) rich text to HTML on your
own site. Colophon stores rich text as validated JSON (the `colophon/rich-text` v1 format), never
as HTML; this package turns a stored document into safe HTML exactly as the Studio's preview and
Colophon's own sites do. It has no dependencies and runs in Node 22.17+, browsers and edge
runtimes.

```ts
import { Client } from '@colophon-cms/sdk';
import { renderToHtml, type RichTextValue } from '@colophon-cms/rich-text';

const origin = 'https://cms.example.com';
const page = await new Client({ baseUrl: origin }).delivery('my-site').list('articles', { limit: 10 });

for (const entry of page.data) {
  const body = entry.content?.['body'] as RichTextValue | undefined;
  if (body === undefined) continue;
  const html = renderToHtml(body, {
    // Images name media-library assets; serve them from Colophon's media route.
    assetUrl: (assetId) => `${origin}/media/v1/my-site/production/${assetId}`
  });
  console.log(entry.title, html.length);
}
```

## What you get

- `renderToHtml(value, { assetUrl })` — the HTML for one stored document: paragraphs, headings
  (`h2`–`h4`), lists, quotes, code blocks (`class="language-…"`), callouts
  (`<aside class="callout callout-note|tip|warning" role="note">`), images in `<figure>`, rules,
  hard breaks, and the bold, italic, strike, code and link marks.
- `UnrenderableContent` — thrown for anything outside the format. Documents reach a site only
  after the server validated them, so this means a bug, never content to guess at.
- `RENDERER_VERSION` — changes whenever the HTML for the same document changes.
- `RICH_TEXT_SCHEMA`, `RICH_TEXT_VERSION`, and the `RichTextValue`, `StoredNode`, `StoredMark`
  types.

## Safety

The output contains only markup the renderer built itself: text and attributes are escaped,
links are limited to `https:`, `http:`, `mailto:`, site paths and fragments, and nothing needs
inline styles or scripts, so it works under a strict Content Security Policy. Style it with your
own CSS.

## Versions

Versioned with the server and the SDK (one version for everything, D-062). Changes are recorded
in the repository's [CHANGELOG.md](https://github.com/akira-shirase-oss/colophon-cms/blob/main/CHANGELOG.md).
MIT licensed.
