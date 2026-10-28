// The seven field types of step M.1 (D-064) in the Studio's content-type builder: offered in plain
// words, with markdown's limit, text's minimum and help text on any field, saved as the server
// writes definitions (help text and minimum only when set).

import { expect, test } from '@playwright/test';
import { api, env, newProject, shot, signIn, violations } from './helpers.ts';

test('an owner builds a content type with the seven new kinds of field, help text and limits', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings`);
	await page.locator('#type-name').fill('Venues');
	await page.getByRole('button', { name: 'Create and add fields' }).click();
	await expect(page.getByTestId('field-title')).toBeVisible();
	const kinds = page.getByLabel('Type');
	const offered = (await kinds.locator('option').allTextContents()).map((name) => name.trim());
	for (const name of ['Email address', 'Web address', 'Date', 'Time of day', 'Structured data (JSON)', 'Markdown text', 'Colour']) {
		expect(offered, name).toContain(name);
	}
	const add = async (label: string, type: string) => {
		await page.getByLabel('New field').fill(label);
		await kinds.selectOption(type);
		await page.getByRole('button', { name: 'Add field' }).click();
		await expect(page.getByTestId(`field-${label.toLowerCase().replaceAll(' ', '_')}`)).toBeVisible();
	};
	await add('Contact email', 'email');
	await add('Website', 'url');
	await add('Opens on', 'date');
	await add('Opens at', 'time');
	await add('Settings', 'json');
	await add('Notes', 'markdown');
	await add('Accent', 'color');
	await page.getByTestId('field-title').getByLabel('At least').fill('3');
	await page.getByTestId('field-notes').getByLabel('Up to').fill('60');
	await page.getByTestId('field-website').getByTestId('field-description').fill('Where readers book a table ');
	await expect(page.getByTestId('field-settings')).toContainText('Structured data (JSON)');
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Venues, version 2')).toBeVisible();
	await shot(page, 'builder-m1');

	const saved = await api(page, 'GET', `${env(project)}/content-types/venues`);
	const fields = (saved.body?.['data'].definition.fields ?? []) as Record<string, unknown>[];
	expect(fields.map((field) => field['type'])).toEqual(['text', 'email', 'url', 'date', 'time', 'json', 'markdown', 'color']);
	expect(fields.find((field) => field['id'] === 'title')).toMatchObject({ min_chars: 3 });
	expect(fields.find((field) => field['id'] === 'notes')).toMatchObject({ max_chars: 60 });
	expect(fields.find((field) => field['id'] === 'website')).toMatchObject({ description: 'Where readers book a table' });
	expect(fields.find((field) => field['id'] === 'accent')).not.toHaveProperty('description');
});
