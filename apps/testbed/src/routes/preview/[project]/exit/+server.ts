// "Exit preview": clears the preview cookie and goes back to the page (`?path=`), now published.

import { exitPreviewHandler } from '@colophon-cms/sdk';
import type { RequestHandler } from './$types';
import { sitePrefix } from '../../site.server.ts';

export const prerender = false;

export const GET: RequestHandler = ({ request, params }) =>
	exitPreviewHandler({ isAllowedPath: (path) => path.startsWith(sitePrefix(params.project)) })(request);
