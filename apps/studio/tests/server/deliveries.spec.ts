// Deliveries in the Studio against a real server and worker (D-046): an owner adds a webhook
// destination on the Releases page, publishes, and sees the release delivered; the receiver got
// exactly one announcement, with an idempotency key. A CDN destination (D-050) is asked to purge
// by tag, and the release says once the purge is accepted.

import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

interface Seen {
	key: string;
	body: { generation: number; entries: { title: string }[] };
}

/** A webhook receiver on 127.0.0.1 that records what it gets and answers 200. */
async function receiver(): Promise<{ url: string; seen: Seen[]; close: () => void }> {
	const seen: Seen[] = [];
	const server = createServer((request, response) => {
		let body = '';
		request.on('data', (chunk: Buffer) => (body += chunk.toString()));
		request.on('end', () => {
			seen.push({ key: String(request.headers['idempotency-key'] ?? ''), body: JSON.parse(body) as Seen['body'] });
			response.writeHead(200).end();
		});
	});
	await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
	const { port } = server.address() as AddressInfo;
	return { url: `http://127.0.0.1:${port}/hook`, seen, close: () => server.close() };
}

test('a destination added on the Releases page gets each release, once', async ({ page }) => {
	const hook = await receiver();
	try {
		await signIn(page);
		const project = await newProject(page);
		await page.goto(`/studio/projects/${project}/releases`);
		await page.getByLabel('Destination name').fill('Site rebuild');
		await page.getByLabel('Webhook URL (https)').fill(hook.url);
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('destinations')).toContainText('Site rebuild');
		expect(await violations(page)).toEqual([]);

		// An approved changeset of one article, published through the API.
		const published = await page.evaluate(async (project) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const key = () => crypto.randomUUID().replaceAll('-', '');
			const base = `/api/v1/projects/${project}/environments/production`;
			const body = { title: 'Delivered', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify({ title: 'Delivered', entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			const reply = await fetch(`${base}/changesets/${set.id}/publish`, {
				method: 'POST',
				headers: { ...ifMatch, 'Idempotency-Key': key() },
				body: JSON.stringify({ base_generation: 0 })
			});
			return reply.status;
		}, project);
		expect(published).toBe(201);

		await page.reload();
		await expect(page.getByTestId('delivery')).toHaveAttribute('data-status', 'succeeded', { timeout: 15_000 });
		await expect(page.getByTestId('delivery')).toContainText('Site rebuild: delivered');
		expect(hook.seen).toHaveLength(1);
		expect(hook.seen[0]?.key).toMatch(/^colophon:/);
		expect(hook.seen[0]?.body.entries[0]?.title).toBe('Delivered');
	} finally {
		hook.close();
	}
});

/** A CDN's purge endpoint on 127.0.0.1 that records the tags of each purge and answers 200. */
async function purger(): Promise<{ url: string; purges: string[][]; close: () => void }> {
	const purges: string[][] = [];
	const server = createServer((request, response) => {
		let body = '';
		request.on('data', (chunk: Buffer) => (body += chunk.toString()));
		request.on('end', () => {
			purges.push((JSON.parse(body) as { tags: string[] }).tags);
			response.writeHead(200).end();
		});
	});
	await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
	const { port } = server.address() as AddressInfo;
	return { url: `http://127.0.0.1:${port}/purge`, purges, close: () => server.close() };
}

test('a CDN added on the Releases page is asked to purge what each release changed', async ({ page }) => {
	const cdn = await purger();
	try {
		await signIn(page);
		const project = await newProject(page);
		await page.goto(`/studio/projects/${project}/releases`);
		await page.getByLabel('Kind').selectOption('cdn');
		await page.getByLabel('Destination name').fill('Edge cache');
		await page.getByLabel('Purge URL (https)').fill(cdn.url);
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('destinations')).toContainText('Edge cache');
		await expect(page.getByTestId('destinations')).toContainText('Purge on release');
		expect(await violations(page)).toEqual([]);

		const entry = await page.evaluate(async (project) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const key = () => crypto.randomUUID().replaceAll('-', '');
			const base = `/api/v1/projects/${project}/environments/production`;
			const body = { title: 'Cached', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const created = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify(body) });
			const entryId = ((await created.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify({ title: 'Cached', entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			await fetch(`${base}/changesets/${set.id}/publish`, { method: 'POST', headers: { ...ifMatch, 'Idempotency-Key': key() }, body: JSON.stringify({ base_generation: 0 }) });
			return entryId;
		}, project);

		await page.reload();
		await expect(page.getByTestId('purge')).toHaveAttribute('data-state', 'accepted', { timeout: 15_000 });
		await expect(page.getByTestId('purge')).toHaveText('CDN purge accepted');
		expect(cdn.purges).toHaveLength(1);
		expect(cdn.purges[0]?.some((tag) => tag.endsWith(`:n:${entry.replaceAll('-', '')}`))).toBe(true);
		expect(cdn.purges[0]?.some((tag) => tag.endsWith(':c:articles'))).toBe(true);
	} finally {
		cdn.close();
	}
});
