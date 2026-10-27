import type { RequestHandler } from './$types';

export const prerender = true;
export const trailingSlash = 'never';

export const GET: RequestHandler = ({ url }) =>
	new Response(`User-agent: *\nAllow: /\n\nSitemap: ${new URL('/sitemap.xml', url.origin).href}\n`, {
		headers: { 'Content-Type': 'text/plain; charset=utf-8' }
	});
