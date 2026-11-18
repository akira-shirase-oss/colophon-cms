// The preview handler — the URL the Studio's preview target names. It trades the one-time code
// for a token, keeps the token in the site's own cookie and redirects to `path` (only ever a page
// of this site); a used or expired code gets the SDK's small 404 page.

import { createPreviewHandler } from '@colophon-cms/sdk';
import type { RequestHandler } from './$types';
import { CMS, sitePrefix } from '../../site.server.ts';

export const prerender = false;

export const GET: RequestHandler = ({ request, params }) =>
	createPreviewHandler({
		baseUrl: CMS,
		project: params.project,
		isAllowedPath: (path) => path.startsWith(sitePrefix(params.project))
	})(request);
