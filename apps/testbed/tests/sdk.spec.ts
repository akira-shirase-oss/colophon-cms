// The npm packages against the real server (D-170): @colophon-cms/sdk and @colophon-cms/rich-text
// packed exactly as npm would ship them, installed in a scratch project, and used from Node the
// way a site's build or server would — no cookie, no token. A person publishes through the
// browser; the packed SDK checks the server's version, reads the release, renders its rich text,
// and pins a page's reads to one generation while another release lands between them (K1).

import { execFileSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

const repo = fileURLToPath(new URL('../../../', import.meta.url));
const ORIGIN = `http://localhost:${process.env['COLOPHON_TESTBED_SERVER_PORT'] ?? 58091}`;

/* The packed packages' surface this test uses, typed here: the tarballs are installed at run time. */
interface Meta {
	status: number;
	generation: number | null;
}
interface Entry {
	id?: string;
	title?: string;
	content?: Record<string, unknown>;
}
interface Delivery {
	list(collection: string, opts?: { limit?: number }): Promise<{ data: Entry[]; response: Meta }>;
	get(collection: string, entry: string): Promise<{ data: Entry; response: Meta }>;
	atGeneration(generation: number): Delivery;
}
interface Sdk {
	SDK_VERSION: string;
	Client: new (options: { baseUrl: string; compat?: 'off' | 'warn' | 'throw' }) => {
		compatibility(): Promise<{ compatible: boolean; sdk: string; server: string | null; reason?: string }>;
		delivery(project: string): Delivery;
	};
}
interface RichText {
	renderToHtml(value: unknown, options: { assetUrl: (id: string) => string }): string;
}

let scratch = '';
let sdk: Sdk;
let richText: RichText;

test.beforeAll(() => {
	scratch = mkdtempSync(path.join(tmpdir(), 'colophon-testbed-sdk-'));
	const tarballs = ['packages/sdk', 'packages/rich-text'].map((dir) => {
		const [packed] = JSON.parse(execFileSync('npm', ['pack', '--json', '--pack-destination', scratch], { cwd: path.join(repo, dir), encoding: 'utf8' })) as { filename: string }[];
		return path.join(scratch, packed!.filename);
	});
	const consumer = path.join(scratch, 'consumer');
	mkdirSync(consumer);
	writeFileSync(path.join(consumer, 'package.json'), '{"name":"site","private":true,"type":"module"}');
	execFileSync('npm', ['install', '--no-audit', '--no-fund', '--ignore-scripts', '--no-package-lock', ...tarballs], { cwd: consumer, stdio: 'ignore' });
});

test.beforeAll(async () => {
	const installed = (name: string) => pathToFileURL(path.join(scratch, 'consumer/node_modules', name, 'dist/index.js')).href;
	sdk = (await import(installed('@colophon-cms/sdk'))) as Sdk;
	richText = (await import(installed('@colophon-cms/rich-text'))) as RichText;
});

test.afterAll(() => {
	if (scratch !== '') rmSync(scratch, { recursive: true, force: true });
});

/** A project with two approved changesets, one article each, whose bodies say `First words.` and
 *  `Second words.`; the project id and the changesets' ids and hashes. */
async function setup(page: Page) {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		async function ok(response: Promise<Response>, what: string): Promise<Response> {
			const answer = await response;
			if (!answer.ok) throw new Error(`setup: ${what}: HTTP ${answer.status} ${await answer.text()}`);
			return answer;
		}
		const created = await ok(fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'SDK testbed' }) }), 'project');
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const base = `/server/api/v1/projects/${project}/environments/production`;
		const sets: { id: string; hash: string }[] = [];
		for (const [title, words] of [
			['First story', 'First words.'],
			['Second story', 'Second words.']
		] as const) {
			const body = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: words }] }] } };
			const entry = await ok(fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title, body }) }), `${title}: entry`);
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await ok(fetch(`${base}/changesets`, { method: 'POST', headers: { ...json, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title, entries: [entryId] }) }), `${title}: changeset`);
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...json, 'If-Match': `"${set.hash}"` };
			await ok(fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch }), `${title}: submit`);
			await ok(fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) }), `${title}: approve`);
			sets.push(set);
		}
		return { project, sets };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

/** Publishes an approved changeset on `baseGeneration`, as the signed-in person. */
async function publish(page: Page, project: string, set: { id: string; hash: string }, baseGeneration: number) {
	const status = await page.evaluate(
		async ({ project, set, baseGeneration }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(`/server/api/v1/projects/${project}/environments/production/changesets/${set.id}/publish`, {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, 'If-Match': `"${set.hash}"`, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ base_generation: baseGeneration })
			});
			return response.status;
		},
		{ project, set, baseGeneration }
	);
	expect(status).toBe(201);
}

test('the packed SDK reads, renders and pins a release on the real server', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project, sets } = await setup(page);
	await publish(page, project, sets[0]!, 0);

	// Anonymous, as a site is; 'throw' so a misfit would fail every call below.
	const client = new sdk.Client({ baseUrl: ORIGIN, compat: 'throw' });
	const fit = await client.compatibility();
	expect(fit, fit.reason).toMatchObject({ compatible: true, sdk: sdk.SDK_VERSION });

	const site = client.delivery(project);
	const first = await site.list('articles', { limit: 10 });
	expect(first.response.generation).toBe(1);
	expect(first.data.map((entry) => entry.title)).toEqual(['First story']);
	const html = richText.renderToHtml(first.data[0]!.content?.['body'], { assetUrl: (id) => `${ORIGIN}/media/v1/${project}/production/${id}` });
	expect(html).toBe('<p>First words.</p>');

	// A page pins the generation its first read answered from; a release landing mid-page moves
	// the latest reads, never the pinned ones (K1).
	const pinned = site.atGeneration(first.response.generation!);
	await publish(page, project, sets[1]!, 1);
	const latest = await site.list('articles', { limit: 10 });
	expect(latest.response.generation).toBe(2);
	expect(latest.data.map((entry) => entry.title).sort()).toEqual(['First story', 'Second story']);
	const still = await pinned.list('articles', { limit: 10 });
	expect(still.response.generation).toBe(1);
	expect(still.data.map((entry) => entry.title)).toEqual(['First story']);
	const one = await pinned.get('articles', first.data[0]!.id!);
	expect(one.response.generation).toBe(1);
	expect(one.data.title).toBe('First story');
});
