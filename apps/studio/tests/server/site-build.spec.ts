// The site build (W.3) end to end against the real server: the website starter's seeded entries
// publish in one release, `build` reads it pinned and writes the page at its slug, the menu's
// resolved link, the theme as the linked stylesheet and the release in every page — and `promote`
// switches the live link atomically.

import { mkdtemp, readFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { build } from '../../../web/src/build.ts';
import { liveDir, promote } from '../../../web/src/promote.ts';
import { api, env, newProject, signIn } from './helpers.ts';

test('a published site builds and promotes atomically', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);

	await page.goto(`/studio/projects/${project}/settings/types`);
	await page.getByTestId('site-template').click();
	await page.getByTestId('site-install').click();
	await expect(page.getByTestId('site-open-home')).toBeVisible();

	// The three starter entries, published in one release through a changeset — exactly what the
	// Studio's publish button does.
	const entries = (collection: string) => api(page, 'GET', `${env(project)}/collections/${collection}/entries`);
	const ids: string[] = [];
	for (const collection of ['pages', 'navigation', 'theme']) {
		const list = ((await entries(collection)).body?.['data'] ?? []) as { id: string }[];
		expect(list.length).toBe(1);
		ids.push(list[0].id);
	}
	const base = `${env(project)}/changesets`;
	const set = await api(page, 'POST', base, { title: 'First release', entries: ids }, { 'Idempotency-Key': `e2e-set-${Date.now()}` });
	expect(set.status).toBe(201);
	const changeset = set.body?.['data'] as { id: string; hash: string };
	const path = `${base}/${changeset.id}`;
	expect((await api(page, 'POST', `${path}/submit`, undefined, { 'If-Match': `"${changeset.hash}"` })).status).toBe(200);
	const reviewed = await api(page, 'POST', `${path}/reviews`, { decision: 'approve' }, { 'If-Match': `"${changeset.hash}"` });
	expect(reviewed.status).toBe(200);
	const hash = (reviewed.body?.['data'] as { hash: string }).hash;
	const published = await api(
		page,
		'POST',
		`${path}/publish`,
		{ base_generation: 0 },
		{ 'If-Match': `"${hash}"`, 'Idempotency-Key': `e2e-publish-${Date.now()}` }
	);
	expect(published.status, JSON.stringify(published.body)).toBe(201);
	const generation = (published.body?.['data'] as { generation: number }).generation;
	expect(generation).toBe(1);

	// The build itself, against this server's public delivery API.
	const root = await mkdtemp(join(tmpdir(), 'colophon-site-'));
	const built = await build({ api: new URL(page.url()).origin, project, environment: 'production', root });
	expect(built.generation).toBe(1);
	expect(await promote(root, built)).toEqual({ promoted: true, generation: 1 });
	const live = await liveDir(root);
	expect(live).toBe(built.dir);

	const index = await readFile(join(root, 'current', 'index.html'), 'utf8');
	const home = await readFile(join(root, 'current', 'home', 'index.html'), 'utf8');
	const css = await readFile(join(root, 'current', 'site.css'), 'utf8');
	expect(index).toContain('Your site, your way'); // first menu item's page is the front page
	expect(index).toContain('href="/home/"');
	expect(home).toBe(index);
	expect(home).toContain('content="1"'); // every page marked with its release
	expect(home).toContain('Read more');
	expect(css).toContain('--site-primary:#6d28d9');
});
