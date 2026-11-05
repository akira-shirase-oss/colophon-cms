// The shared piece of the five Astro demo sites (docs/modules/06-delivery.md, apps/sites/README.md):
// which project to read (`site`), anonymous delivery reads (`entries`), and rich text rendered by
// the same renderer the Studio preview uses, so a site can't drift from what the CMS accepted.
export { site, unseeded, type Site } from './config.ts';
export { entries, entryPath, mediaUrl, type Entry } from './delivery.ts';
export { richText } from './richtext.ts';
