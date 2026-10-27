// The build log as RSS 2.0, prerendered. Each item links its original at 404human.dev, the post's
// canonical address; the channel links this site's blog.
import { posts } from '#lib/server/blog.ts';
import type { RequestHandler } from './$types';

export const prerender = true;
export const trailingSlash = 'never';

const escape = (text: string) =>
	text.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;').replace(/"/g, '&quot;').replace(/'/g, '&apos;');

export const GET: RequestHandler = ({ url }) => {
	const self = new URL('/blog/rss.xml', url.origin).href;
	const items = posts
		.map(
			(post) => `    <item>
      <title>${escape(post.title)}</title>
      <link>${escape(post.canonical)}</link>
      <guid isPermaLink="true">${escape(post.canonical)}</guid>
      <pubDate>${new Date(post.date).toUTCString()}</pubDate>
      <category>${escape(`${post.milestone} · ${post.step}`)}</category>
      <description>${escape(post.description)}</description>
    </item>`
		)
		.join('\n');
	const newest = posts[0]?.date ?? new Date(0).toISOString();
	const body = `<?xml version="1.0" encoding="UTF-8"?>
<rss version="2.0" xmlns:atom="http://www.w3.org/2005/Atom">
  <channel>
    <title>Colophon build log</title>
    <link>${escape(new URL('/blog/', url.origin).href)}</link>
    <atom:link href="${escape(self)}" rel="self" type="application/rss+xml" />
    <description>Every step of Colophon CMS, written up as it happened, mistakes included.</description>
    <language>en</language>
    <lastBuildDate>${new Date(newest).toUTCString()}</lastBuildDate>
${items}
  </channel>
</rss>
`;
	return new Response(body, { headers: { 'Content-Type': 'application/rss+xml; charset=utf-8' } });
};
