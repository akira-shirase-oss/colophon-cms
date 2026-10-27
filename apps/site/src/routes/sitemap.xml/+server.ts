// Every page whose canonical address is on this site. Blog posts are left out: each names its
// original at 404human.dev as canonical, and a sitemap lists canonical addresses only.
import type { RequestHandler } from './$types';

export const prerender = true;
export const trailingSlash = 'never';

const PAGES = ['/', '/product/', '/compare/', '/roadmap/', '/changelog/', '/blog/', '/open-source/'];

export const GET: RequestHandler = ({ url }) => {
	const urls = PAGES.map((path) => `  <url><loc>${new URL(path, url.origin).href}</loc></url>`).join('\n');
	return new Response(
		`<?xml version="1.0" encoding="UTF-8"?>
<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">
${urls}
</urlset>
`,
		{ headers: { 'Content-Type': 'application/xml; charset=utf-8' } }
	);
};
