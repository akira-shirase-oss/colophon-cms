// Locales (D-087) through the testbed, in a real browser, against the real server: an English
// note grows a French sibling through the localizations route, the group lists both members,
// a second French version is refused, and the list keeps only the French row.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Locales testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('an entry grows a localization group one tag at a time', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/locales');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByRole('button', { name: 'Set up an English note and its French version' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');
	const entryId = await page.getByTestId('entry-id').textContent();
	const siblingId = await page.getByTestId('sibling-id').textContent();
	expect(entryId).toMatch(/^[0-9a-f-]{36}$/);
	expect(siblingId).toMatch(/^[0-9a-f-]{36}$/);
	expect(siblingId).not.toBe(entryId);

	// The group lists both members: the source's en row and the new fr row.
	await expect(page.getByTestId('group')).toContainText(`en → ${entryId}`);
	await expect(page.getByTestId('group')).toContainText(`fr → ${siblingId}`);

	// A second French version is refused: the locale is taken.
	await page.getByRole('button', { name: 'Localize it in French again' }).click();
	await expect(page.getByTestId('repeat')).toContainText('409');
	await expect(page.getByTestId('repeat')).toContainText('locale_taken');

	// The list's locale filter keeps only the French row.
	await page.getByRole('button', { name: 'List the French entries' }).click();
	await expect(page.getByTestId('french-list')).toHaveText(`fr → ${siblingId}`);
});
