// The overview's Get started checklist, after the hands-on comparison with Strapi (D-082): it leads
// with deciding what you'll write (Content types), done once the project has its own kinds of
// content; "Write your first …" names the first collection the person made, never a one-entry type
// or whichever type is first in the alphabet or got an entry first; and a role that can't change
// content types isn't offered the first step.

import { expect, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, signIn, violations } from './helpers.ts';

const named = (name: string, single = false) => ({
	name,
	title_field: 'title',
	...(single ? { single: true } : {}),
	fields: [{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false }]
});

test('Get started leads with Content types, and writes the first entry of the first collection the person made', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}`);
	const list = page.getByTestId('first-run');

	// A blank project has only the built-in Articles: nothing decided yet.
	const first = list.getByTestId('first-run-item').first();
	await expect(first).toHaveAttribute('data-item', 'types');
	await expect(first).toContainText('Decide what you’ll write (Content types)');
	await expect(first).toHaveAttribute('data-done', 'false');
	await expect(list.getByRole('link', { name: 'Choose: Decide what you’ll write (Content types)' })).toHaveAttribute(
		'href',
		`/studio/projects/${project}/settings/types`
	);
	await expect(list.locator('[data-item=write]')).toContainText('Write your first article');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);

	// Zines first, then a one-entry About page and Bakes; the first entry goes into Bakes.
	for (const [slug, definition] of [
		['zines', named('Zines')],
		['about', named('About', true)],
		['bakes', named('Bakes')]
	] as const) {
		expect((await api(page, 'POST', `${env(project)}/content-types`, { slug, definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	}
	// Your work offers the same, while nothing is written.
	await page.goto('/studio/');
	await expect(page.getByTestId('my-work').getByRole('link', { name: 'Write a new zine' })).toHaveAttribute('href', `/studio/projects/${project}/zines?new=1`);
	await newEntry(page, project, 'Sourdough', 'bakes', { title: 'Sourdough' });
	await page.goto(`/studio/projects/${project}`);
	await expect(list.locator('[data-item=types]')).toHaveAttribute('data-done', 'true');
	await expect(list.locator('[data-item=write]')).toContainText('Write your first zine');
	await expect(list.locator('[data-item=write]')).toHaveAttribute('data-done', 'true');

	// An editor can't change content types, so isn't offered that step.
	const editor = await member(page, browser, project, 'editor');
	await expect(editor.getByTestId('first-run')).toBeVisible();
	await expect(editor.locator('[data-item=types]')).toHaveCount(0);
	await expect(editor.getByTestId('first-run-item').first()).toHaveAttribute('data-item', 'write');
	await editor.context().close();
});
