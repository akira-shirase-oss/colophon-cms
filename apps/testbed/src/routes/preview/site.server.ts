// The site's settings, server-side only. A real site has one project and one Studio; this one
// serves whichever project a test made, so the project travels in the path:
//
//   preview target  {site}/preview/{project}/api?code={code}&path=/preview/{project}/{collection}/{id}
//   pages           {site}/preview/{project}/{collection}/{id}
//   exit            {site}/preview/{project}/exit?path=…

import { env } from 'node:process';

/** Colophon, as this server reaches it (the same server the `/server` proxy names). */
export const CMS = env['COLOPHON_SERVER'] ?? 'http://127.0.0.1:58000';

/** The Studio's exact origin: the only page allowed to frame a preview or message it. */
export const STUDIO_ORIGIN = env['COLOPHON_STUDIO_ORIGIN'] ?? import.meta.env['VITE_COLOPHON_SERVER_ORIGIN'] ?? 'http://localhost:58000';

/** Every page of one project's site starts here; the handlers redirect nowhere else. */
export function sitePrefix(project: string): string {
	return `/preview/${encodeURIComponent(project)}/`;
}
