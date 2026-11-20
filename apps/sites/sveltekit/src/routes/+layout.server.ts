// Every page is prerendered at build time, at the one release this build reads.
import { site } from '@colophon/site-kit';
import { buildRelease } from '#lib/server/colophon.ts';

export const prerender = true;
export const trailingSlash = 'always';

export async function load() {
	const connection = site();
	// Not seeded yet (a clean checkout, CI): draw a page that says so instead of failing the build.
	if (connection === null) return { generation: null };
	return { generation: await buildRelease(connection, 'recipes') };
}
