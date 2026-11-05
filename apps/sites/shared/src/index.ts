// The shared piece of the five Astro demo sites (docs/modules/06-delivery.md, apps/sites/README.md):
// which project to read (`site`), anonymous delivery reads (`entries`), and rich text rendered by
// the same renderer the Studio preview uses, so a site can't drift from what the CMS accepted.
export { site, unseeded, type Site } from './config.ts';
export { entries, entry, entryPath, mediaUrl, type Entry, type Entries } from './delivery.ts';
export { me, signIn, signOut, signUp, type AudienceUser, type CredentialResult, type Issued } from './audience.ts';
export { richText } from './richtext.ts';
