// A publish request says which language each entry in it is written in (D-180): the language
// its pinned revision carries, by the name the project's list gives it — so a reviewer sees which
// readers each change reaches. An entry in the default language says nothing extra.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, newEntry, newProject, signIn, violations } from './helpers.ts';

test("a publish request names each entry's language", async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const set = await api(
		page,
		'PUT',
		`${env(project)}/locales`,
		{ locales: [{ tag: 'en', label: 'English', default: true }, { tag: 'fr', label: 'Français', fallback: ['en'] }] },
		{ 'If-Match': '"locales-0"', 'Idempotency-Key': key() }
	);
	expect(set.status, JSON.stringify(set.body)).toBe(200);
	const body = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };
	const french = await api(page, 'POST', `${env(project)}/collections/articles/entries?locale=fr`, { title: 'Bonjour', body }, { 'Idempotency-Key': key() });
	expect(french.status, JSON.stringify(french.body)).toBe(201);
	const plain = await newEntry(page, project, 'Hello');
	const made = await api(
		page,
		'POST',
		`${env(project)}/changesets`,
		{ title: 'Both', entries: [french.body?.['data'].id as string, plain] },
		{ 'Idempotency-Key': key() }
	);
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	await page.goto(`/studio/projects/${project}/changesets/${made.body?.['data'].id as string}`);
	const items = page.getByTestId('changeset-items');
	await expect(items.locator('li', { hasText: 'Bonjour' }).getByTestId('item-language')).toHaveText(' · Français');
	await expect(items.locator('li', { hasText: 'Hello' }).getByTestId('item-language')).toHaveCount(0);
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);
});
