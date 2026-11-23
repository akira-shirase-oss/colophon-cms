// The Next.js demo's delivery reads (D-220) against a stub of the public delivery API: the release the prebuild
// pinned, every read pinned to it, paging by cursor, and a refusal when an answer names another release. Each stub server is
// closed by t.after(), so a failing assertion can't leave the process running.
//
//   npm test -w @colophon/site-nextjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http';
import type { AddressInfo } from 'node:net';
import { mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { buildRelease, entriesAt } from './colophon.ts';

type Handler = (url: URL, res: ServerResponse) => void;

async function stub(handler: Handler) {
	const seen: string[] = [];
	const server = createServer((req: IncomingMessage, res: ServerResponse) => {
		const url = new URL(req.url ?? '/', 'http://stub');
		seen.push(url.pathname + url.search);
		handler(url, res);
	});
	await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve));
	const { port } = server.address() as AddressInfo;
	const site = { api: `http://127.0.0.1:${port}`, project: 'p1', environment: 'production' };
	return { site, seen, close: () => new Promise<void>((resolve) => server.close(() => resolve())) };
}

const answer = (res: ServerResponse, generation: number, body: unknown) =>
	res.writeHead(200, { 'content-type': 'application/json', 'colophon-generation': String(generation) }).end(JSON.stringify(body));
const recipe = (id: string) => ({ id, title: id, published_in: 1, content: { title: id, slug: id } });

test('every worker reads the release the prebuild pinned, and nothing when nothing is seeded', (t) => {
	const dir = mkdtempSync(join(tmpdir(), 'pin-'));
	const cwd = process.cwd();
	process.chdir(dir);
	t.after(() => {
		process.chdir(cwd);
		rmSync(dir, { recursive: true, force: true });
	});
	assert.equal(buildRelease(), null);
	const site = { api: 'https://cms.example.com', project: 'p1', environment: 'production' };
	writeFileSync('.release.json', JSON.stringify({ site, generation: 7 }));
	assert.deepEqual(buildRelease(), { site, generation: 7 });
});

test('reads ask for the pinned release and follow the cursor', async (t) => {
	const s = await stub((url, res) => {
		if (url.searchParams.get('cursor') === null) answer(res, 7, { data: [recipe('a'), recipe('b')], next_cursor: 'c2' });
		else answer(res, 7, { data: [recipe('c')], next_cursor: null });
	});
	t.after(s.close);
	const items = await entriesAt(s.site, 'recipes', 7);
	assert.deepEqual(items.map((r) => r.id), ['a', 'b', 'c']);
	assert.equal(s.seen.length, 2);
	for (const request of s.seen) assert.match(request, /generation=7/, request);
	assert.match(s.seen[1]!, /cursor=c2/);
});

test('an answer from another release is refused, so a build never mixes two releases', async (t) => {
	const s = await stub((_url, res) => answer(res, 8, { data: [recipe('a')] }));
	t.after(s.close);
	await assert.rejects(entriesAt(s.site, 'recipes', 7), /asked for release 7, got 8/);
});

test('a refused or failed answer fails the build instead of rendering an empty site', async (t) => {
	const s = await stub((_url, res) => res.writeHead(503).end());
	t.after(s.close);
	await assert.rejects(entriesAt(s.site, 'recipes', 1), /HTTP 503/);
});
