// Deliveries through the testbed, in a real browser (D-046): add a webhook destination pointing
// at a receiver in this test, publish a release through the API, and watch its job become
// delivered; a refused address is shown with its reason. A CDN destination (D-050) gets a purge
// by tag instead, and the release shows it accepted. A probe (D-051) verifies a release once its
// site shows it, and a location only probes may have is refused elsewhere.

import { createServer } from 'node:http';
import type { AddressInfo } from 'node:net';
import { expect, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

test('add a destination, publish, and see the job delivered', async ({ page }) => {
	const keys: string[] = [];
	const server = createServer((request, response) => {
		keys.push(String(request.headers['idempotency-key'] ?? ''));
		request.resume();
		request.on('end', () => response.writeHead(204).end());
	});
	await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
	const hook = `http://127.0.0.1:${(server.address() as AddressInfo).port}/hook`;
	try {
		await signIn(page, 'dev-owner');
		const project = await page.evaluate(async (slug) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const created = await fetch('/server/api/v1/projects', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
				body: JSON.stringify({ slug, name: 'Deliveries testbed' })
			});
			return ((await created.json()) as { data: { id: string } }).data.id;
		}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
		await page.goto('/deliveries');
		await page.getByLabel('Project').selectOption(project);

		await page.getByLabel('Destination name').fill('Bad');
		await page.getByLabel('URL', { exact: true }).fill('ftp://example.com/hook');
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('message')).toHaveText('422 validation_failed (invalid_url)');
		await page.getByLabel('Destination name').fill('Receiver');
		await page.getByLabel('URL', { exact: true }).fill(hook);
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('message')).toHaveText('added');
		await expect(page.getByTestId('destinations')).toContainText('Receiver');

		const status = await page.evaluate(async (project) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const base = `/server/api/v1/projects/${project}/environments/production`;
			const body = { title: 'Announced', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title: 'Announced', entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			const published = await fetch(`${base}/changesets/${set.id}/publish`, {
				method: 'POST',
				headers: { ...ifMatch, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ base_generation: 0 })
			});
			return published.status;
		}, project);
		expect(status).toBe(201);

		await expect(async () => {
			await page.getByRole('button', { name: 'Reload' }).click();
			await expect(page.getByTestId('jobs').getByRole('listitem')).toHaveAttribute('data-status', 'succeeded', { timeout: 500 });
		}).toPass({ timeout: 15_000 });
		await expect(page.getByTestId('jobs')).toContainText('generation 1 to Receiver: succeeded, 1 attempt, HTTP 204');
		expect(keys).toHaveLength(1);
	} finally {
		server.close();
	}
});

test('a CDN destination is sent a purge by tag, and the release shows it accepted', async ({ page }) => {
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
	const purgeUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/purge`;
	try {
		await signIn(page, 'dev-owner');
		const project = await page.evaluate(async (slug) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const created = await fetch('/server/api/v1/projects', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
				body: JSON.stringify({ slug, name: 'Purges testbed' })
			});
			return ((await created.json()) as { data: { id: string } }).data.id;
		}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
		await page.goto('/deliveries');
		await page.getByLabel('Project').selectOption(project);
		await page.getByLabel('Kind (webhook or cdn)').fill('mirror');
		await page.getByLabel('Destination name').fill('Edge');
		await page.getByLabel('URL', { exact: true }).fill(purgeUrl);
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('message')).toHaveText('422 validation_failed (invalid_kind)');
		await page.getByLabel('Kind (webhook or cdn)').fill('cdn');
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('destinations')).toContainText('Edge (cdn)');

		const status = await page.evaluate(async (project) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const base = `/server/api/v1/projects/${project}/environments/production`;
			const body = { title: 'Purged', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title: 'Purged', entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			const published = await fetch(`${base}/changesets/${set.id}/publish`, {
				method: 'POST',
				headers: { ...ifMatch, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ base_generation: 0 })
			});
			return published.status;
		}, project);
		expect(status).toBe(201);

		await expect(async () => {
			await page.getByRole('button', { name: 'Reload' }).click();
			await expect(page.getByTestId('jobs').getByRole('listitem')).toHaveAttribute('data-status', 'succeeded', { timeout: 500 });
		}).toPass({ timeout: 15_000 });
		expect(purges).toHaveLength(1);
		expect(purges[0]?.some((tag) => tag.endsWith(':c:articles'))).toBe(true);
		await page.goto('/releases');
		await page.getByLabel('Project').selectOption(project);
		await expect(page.getByTestId('releases')).toContainText('generation 1: Purged (1 changed), purge accepted');
	} finally {
		server.close();
	}
});

test('a probe verifies a release once its site shows it, and the receipt says from where', async ({ page }) => {
	let generation = 0;
	const server = createServer((request, response) => {
		request.resume();
		request.on('end', () =>
			response.writeHead(200, { 'content-type': 'text/html' }).end(`<meta name="colophon-generation" content="${generation}">`)
		);
	});
	await new Promise<void>((done) => server.listen(0, '127.0.0.1', done));
	const pageUrl = `http://127.0.0.1:${(server.address() as AddressInfo).port}/`;
	try {
		await signIn(page, 'dev-owner');
		const project = await page.evaluate(async (slug) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const created = await fetch('/server/api/v1/projects', {
				method: 'POST',
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
				body: JSON.stringify({ slug, name: 'Receipts testbed' })
			});
			return ((await created.json()) as { data: { id: string } }).data.id;
		}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
		await page.goto('/deliveries');
		await page.getByLabel('Project').selectOption(project);
		await page.getByLabel('Kind (webhook or cdn)').fill('webhook');
		await page.getByLabel('Destination name').fill('Pinned hook');
		await page.getByLabel('URL', { exact: true }).fill(pageUrl);
		await page.getByLabel('Location (probes only; empty for any)').fill('eu-west');
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('message')).toHaveText('422 validation_failed (invalid_location)');
		await page.getByLabel('Kind (webhook or cdn)').fill('probe');
		await page.getByLabel('Destination name').fill('Home page');
		await page.getByLabel('Location (probes only; empty for any)').fill('');
		await page.getByRole('button', { name: 'Add destination' }).click();
		await expect(page.getByTestId('destinations')).toContainText('Home page (probe)');

		generation = 1;
		const status = await page.evaluate(async (project) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const base = `/server/api/v1/projects/${project}/environments/production`;
			const body = { title: 'Seen', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': crypto.randomUUID() }, body: JSON.stringify({ title: 'Seen', entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			const published = await fetch(`${base}/changesets/${set.id}/publish`, {
				method: 'POST',
				headers: { ...ifMatch, 'Idempotency-Key': crypto.randomUUID() },
				body: JSON.stringify({ base_generation: 0 })
			});
			return published.status;
		}, project);
		expect(status).toBe(201);

		await page.goto('/releases');
		await page.getByLabel('Project').selectOption(project);
		await page.getByLabel('Receipt of generation').fill('1');
		await expect(async () => {
			await page.getByRole('button', { name: 'Read receipt' }).click();
			await expect(page.getByTestId('receipt')).toContainText('verified at 1', { timeout: 500 });
		}).toPass({ timeout: 15_000 });
		await expect(page.getByTestId('receipt')).toContainText('Home page done');
		await expect(page.getByTestId('receipt')).toContainText('last: local saw 1 (HTTP 200)');
		await page.getByLabel('Receipt of generation').fill('9');
		await page.getByRole('button', { name: 'Read receipt' }).click();
		await expect(page.getByTestId('receipt')).toHaveText('404 not_found');
	} finally {
		server.close();
	}
});
