// `@colophon-cms/rich-text` — the renderer the Studio and Colophon's own sites use, for any site
// that reads Colophon's delivery API. It is compiled together with files `scripts/build.mjs`
// assembles from their one source — the renderer (`packages/content-renderer/src/render.ts`) and
// the format's rules (`packages/editor/src/schema.ts`) — so this package can't drift from what
// the Studio previews (invariant R1).
//
//   import { renderToHtml } from '@colophon-cms/rich-text';
//   const html = renderToHtml(entry.content.body, {
//     assetUrl: (id) => `${origin}/media/v1/${project}/${environment}/${id}`
//   });

export { renderToHtml, RENDERER_VERSION, UnrenderableContent, type RenderOptions } from './render.js';
export { RICH_TEXT_SCHEMA, RICH_TEXT_VERSION } from './schema.js';
export type { RichTextValue, StoredNode, StoredMark } from './stored.js';
