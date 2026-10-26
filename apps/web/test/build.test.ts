import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';

import { BuildFailed, build } from '../src/build.ts';
import { marker } from '../src/pages.ts';
import { api, item } from './colophon.ts';

const settings = { api: 'http://colophon.test', project: 'p1', environment: 'production' };

describe('a build', () => {
	test('reads one release, page after page, and marks every page with it', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const articles = [item('a3', 'Third <b>'), item('a2', 'Second'), item('a1', 'First')];
		const colophon = api(new Map([[6, [item('old', 'Older')]], [7, articles]]), 2);
		const built = await build({ ...settings, root, fetch: colophon.fetch, now: new Date('2026-10-26T12:00:00Z') });
		expect(built.generation).toBe(7);
		expect(colophon.asked.map((url) => url.search)).toEqual(['?limit=100', '?limit=100&cursor=7%3A2']);
		expect(built.pages.sort()).toEqual(['/404.html', '/articles/a1/index.html', '/articles/a2/index.html', '/articles/a3/index.html', '/colophon/index.html', '/index.html']);
		for (const page of built.pages) {
			const html = await readFile(join(built.dir, page), 'utf8');
			expect(html.split(marker(7))).toHaveLength(2);
		}
		const home = await readFile(join(built.dir, 'index.html'), 'utf8');
		expect(home).toContain('Third &lt;b&gt;');
		expect(home).not.toContain('<b>');
		expect(await readFile(join(built.dir, 'articles/a2/index.html'), 'utf8')).toContain('<p>Body of Second</p>');
		expect(await readFile(join(built.dir, 'colophon/index.html'), 'utf8')).toContain('release <strong>7</strong>');
		expect((await readFile(join(built.dir, 'generation.txt'), 'utf8')).trim()).toBe('7');
	});

	test('pins every read to the release asked for', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const colophon = api(new Map([[3, [item('a', 'Then')]], [9, [item('b', 'Now')]]]));
		const built = await build({ ...settings, generation: 3, root, fetch: colophon.fetch });
		expect(built.generation).toBe(3);
		expect(colophon.asked[0]?.searchParams.get('generation')).toBe('3');
		expect(await readFile(join(built.dir, 'index.html'), 'utf8')).toContain('Then');
	});

	test('fails rather than mix releases, guess at content or build from an error', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const mixed = (async () =>
			new Response(JSON.stringify({ data: [], next_cursor: null }), { headers: { 'colophon-generation': '8' } })) as typeof fetch;
		await expect(build({ ...settings, generation: 7, root, fetch: mixed })).rejects.toThrow(BuildFailed);
		const broken = (async () => new Response('{}', { status: 503 })) as typeof fetch;
		await expect(build({ ...settings, root, fetch: broken })).rejects.toThrow('HTTP 503');
		const strange = item('x', 'Strange');
		strange.content.body = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'script' }] } };
		await expect(build({ ...settings, root, fetch: api(new Map([[1, [strange]]])).fetch })).rejects.toThrow();
	});
});
