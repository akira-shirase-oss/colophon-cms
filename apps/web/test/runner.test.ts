import { mkdtemp } from 'node:fs/promises';
import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { afterEach, describe, expect, test } from 'vitest';

import { live } from '../src/promote.ts';
import { Runner } from '../src/runner.ts';
import { api, item } from './colophon.ts';

const servers: Array<{ close: () => void }> = [];
afterEach(() => {
	for (const server of servers.splice(0)) server.close();
});

async function start(releases: Map<number, ReturnType<typeof item>[]>) {
	const root = await mkdtemp(join(tmpdir(), 'site-'));
	const runner = new Runner({ api: 'http://colophon.test', project: 'p', environment: 'production', root, fetch: api(releases).fetch });
	const server = createServer((request, response) => void runner.handle(request, response));
	await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
	servers.push(server);
	return { root, runner, url: `http://127.0.0.1:${(server.address() as AddressInfo).port}` };
}

async function until(check: () => Promise<boolean>): Promise<void> {
	const deadline = Date.now() + 5_000;
	while (!(await check())) {
		if (Date.now() > deadline) throw new Error('timed out');
		await new Promise((resume) => setTimeout(resume, 20));
	}
}

describe('the site runner', () => {
	test('builds the newest release it hears about and serves it', async () => {
		const releases = new Map([
			[1, [item('a1', 'One')]],
			[2, [item('a2', 'Two'), item('a1', 'One')]]
		]);
		const site = await start(releases);
		expect((await fetch(`${site.url}/`)).status).toBe(503);
		for (const body of ['nope', '{"generation":0}', '{"generation":"2"}']) {
			expect((await fetch(`${site.url}/hook`, { method: 'POST', body })).status).toBe(400);
		}
		expect((await fetch(`${site.url}/hook`, { method: 'POST', body: '{"generation":1}' })).status).toBe(202);
		expect((await fetch(`${site.url}/hook`, { method: 'POST', body: '{"generation":2}' })).status).toBe(202);
		await until(async () => (await live(site.root)) === 2);
		const home = await fetch(`${site.url}/`);
		expect(home.status).toBe(200);
		const html = await home.text();
		expect(html).toContain('<meta name="colophon-generation" content="2">');
		expect(html).toContain('Two');
		expect((await fetch(`${site.url}/articles/a2/`)).status).toBe(200);
		const missing = await fetch(`${site.url}/articles/nothing/`);
		expect(missing.status).toBe(404);
		expect(await missing.text()).toContain('Not found');
	});

	test('serves nothing from outside the live site', async () => {
		const site = await start(new Map([[1, [item('a1', 'One')]]]));
		site.runner.announce(1);
		await until(async () => (await live(site.root)) === 1);
		for (const path of ['/../../../etc/passwd', '/%2e%2e/%2e%2e/etc/passwd', '/..%2fbuilds']) {
			const response = await fetch(`${site.url}${path}`);
			expect(response.status, path).toBe(404);
		}
		expect((await fetch(`${site.url}/hook`, { method: 'PUT' })).status).toBe(405);
	});
});

describe('a runner without a project', () => {
	test('adopts the first announced project and refuses others', async () => {
		const site = await start(new Map([[1, [item('a1', 'One')]]]));
		site.runner.settings.project = '';
		const announce = (project: string) =>
			fetch(`${site.url}/hook`, { method: 'POST', body: JSON.stringify({ generation: 1, project }) });
		expect((await fetch(`${site.url}/hook`, { method: 'POST', body: '{"generation":1}' })).status).toBe(400);
		expect((await announce('p-first')).status).toBe(202);
		expect((await announce('p-other')).status).toBe(400);
		expect(site.runner.settings.project).toBe('p-first');
		await until(async () => (await live(site.root)) === 1);
	});
});
