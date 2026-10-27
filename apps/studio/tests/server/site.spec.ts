// The whole path, in a browser (step 3.4, D-052): an article approved and published in the
// Studio is announced to the reference site's runner (apps/web), which builds that release into
// a static site and promotes it; a probe reads the live page's marker, so the release's receipt
// says "Verified at 1 location"; and the live site itself reads well and passes the
// accessibility checks.

import { type ChildProcess, spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import { newProject, signIn, violations } from './helpers.ts';

const repo = fileURLToPath(new URL('../../../../', import.meta.url));
// COLOPHON_STUDIO_SITE_PORT moves it, like the server's port, so two checkouts can run at once.
const SITE_LISTEN = `127.0.0.1:${process.env['COLOPHON_STUDIO_SITE_PORT'] ?? '58094'}`;
const SITE = `http://${SITE_LISTEN}`;

/** Starts the site runner for `project`; stop it and remove its folder with the returned function. */
function runner(project: string, api: string): () => void {
	const root = mkdtempSync(join(tmpdir(), 'colophon-site-'));
	const child: ChildProcess = spawn(process.execPath, [join(repo, 'apps/web/src/runner.ts')], {
		cwd: repo,
		env: { ...process.env, SITE_API: api, SITE_PROJECT: project, SITE_ROOT: root, SITE_LISTEN },
		stdio: 'ignore'
	});
	return () => {
		child.kill();
		rmSync(root, { recursive: true, force: true });
	};
}

test('an article published in the Studio is live on the site, verified, and accessible', async ({ page, baseURL }) => {
	await signIn(page);
	const project = await newProject(page);
	const stop = runner(project, baseURL ?? 'http://localhost:58093');
	try {
		await expect(async () => {
			const home = await page.request.get(`${SITE}/`);
			expect(home.status()).toBe(200);
			expect(await home.text()).toContain('Nothing published yet');
		}).toPass({ timeout: 20_000 });

		// The site hears about releases, and a probe checks the live home page.
		await page.goto(`/studio/projects/${project}/settings`);
		await page.getByLabel('Connection name').fill('Site build');
		await page.getByLabel('Webhook URL (https)').fill(`${SITE}/hook`);
		await page.getByRole('button', { name: 'Add website connection' }).click();
		await expect(page.getByTestId('destinations')).toContainText('Site build');
		await page.getByLabel('Kind').selectOption('probe');
		await page.getByLabel('Connection name').fill('Live site');
		await page.getByLabel('URL to check (https)').fill(`${SITE}/`);
		await page.getByRole('button', { name: 'Add website connection' }).click();
		await expect(page.getByTestId('destinations')).toContainText('Live site');

		// An approved changeset of one article, made through the API; published in the Studio.
		const changeset = await page.evaluate(async (project) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const headers = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const key = () => crypto.randomUUID().replaceAll('-', '');
			const base = `/api/v1/projects/${project}/environments/production`;
			const body = {
				title: 'Hello from Colophon',
				body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Built from one release.' }] }] } }
			};
			const entry = await fetch(`${base}/collections/articles/entries`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify(body) });
			const entryId = ((await entry.json()) as { data: { id: string } }).data.id;
			const made = await fetch(`${base}/changesets`, { method: 'POST', headers: { ...headers, 'Idempotency-Key': key() }, body: JSON.stringify({ title: 'First post', entries: [entryId] }) });
			const set = ((await made.json()) as { data: { id: string; hash: string } }).data;
			const ifMatch = { ...headers, 'If-Match': `"${set.hash}"` };
			await fetch(`${base}/changesets/${set.id}/submit`, { method: 'POST', headers: ifMatch });
			await fetch(`${base}/changesets/${set.id}/reviews`, { method: 'POST', headers: ifMatch, body: JSON.stringify({ decision: 'approve' }) });
			return set.id;
		}, project);
		await page.goto(`/studio/projects/${project}/changesets/${changeset}`);
		await page.getByRole('button', { name: 'Publish', exact: true }).click();
		await expect(page.getByTestId('release-link')).toHaveText('release 1');

		// The receipt: the site built it and the probe saw it there.
		await page.getByTestId('release-link').click();
		const label = page.locator('[data-generation="1"]').getByTestId('label');
		await expect(label).toHaveText('Verified at 1 location', { timeout: 30_000 });
		await label.click();
		await expect(page.getByTestId('deliveries')).toContainText('Site build');
		await expect(page.getByTestId('deliveries')).toContainText('saw this release');

		// The live site, as a reader gets it.
		await page.goto(`${SITE}/`);
		await expect(page.locator('meta[name="colophon-generation"]')).toHaveAttribute('content', '1');
		expect(await violations(page)).toEqual([]);
		await page.getByRole('link', { name: 'Hello from Colophon' }).click();
		await expect(page.getByRole('heading', { level: 1 })).toHaveText('Hello from Colophon');
		await expect(page.getByText('Built from one release.')).toBeVisible();
		expect(await violations(page)).toEqual([]);
		await page.getByRole('link', { name: 'Colophon', exact: true }).click();
		await expect(page.locator('main')).toContainText('This site is release 1 of its content');
		expect(await violations(page)).toEqual([]);
	} finally {
		stop();
	}
});
