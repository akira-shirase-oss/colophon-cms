// Content modelling through the testbed (steps M.1 to M.4, D-064 to D-066, D-069 to D-073):
// types with an object, a list of lowercase tags and blocks whose quote names a person, whose name
// is unique, whose role defaults to member and whose editor date applies to editors only; the page is stored in canonical form, a quote by nobody, a tag in capitals and a second
// Ada are refused where they are, the page can't be published before the person, both
// together can, and the person's read lists what references them.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Modelling testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('structures are stored canonically, references are checked, and publishing keeps them whole', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/modelling');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByRole('button', { name: 'Set up types and Ada' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');
	// Ada was sent without a role: the default fills it (D-071).
	await expect(page.getByTestId('ada')).toHaveText('{"name":"Ada","role":"member"}');

	await page.getByRole('button', { name: 'Save a page quoting Ada' }).click();
	await expect(page.getByTestId('message')).toHaveText('saved');
	// Fields in definition order, `_block` first in each block, whatever order they were sent in.
	await expect(page.getByTestId('stored')).toHaveText(
		/^\{"title":"Home","hero":\{"heading":"Welcome"\},"tags":\["news"\],"sections":\[\{"_block":"text_block","body":"# Hello"\},\{"_block":"quote","text":"Ship it","by":"[0-9a-f-]{36}"\}\]\}$/
	);

	await page.getByRole('button', { name: 'Save a quote by nobody' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (reference_not_found at /sections/1/by)');
	await page.getByRole('button', { name: 'Save a tag in capitals' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (pattern_mismatch at /tags/0)');
	await page.getByRole('button', { name: 'Add a second Ada' }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (value_taken at /name)');
	await page.getByRole('button', { name: 'Give a member an editor date' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (not_applicable at /since)');
	await page.getByRole('button', { name: 'Widen the SEO component' }).click();
	await expect(page.getByTestId('message')).toHaveText('changed: pages v2');

	await page.getByRole('button', { name: 'Publish the page alone' }).click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (references_unpublished)');
	await page.getByRole('button', { name: 'Publish the page and Ada' }).click();
	await expect(page.getByTestId('message')).toHaveText('published');

	await page.getByRole('button', { name: 'Read Ada' }).click();
	await expect(page.getByTestId('referenced-by')).toHaveText('pages sections/1/by');
});
