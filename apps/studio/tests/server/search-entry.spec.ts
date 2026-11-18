// Search has one home: the header. The sidebar used to repeat it as a menu item; it holds
// navigation only now. On a phone, where the wide header button doesn't fit and there is no ⌘K,
// the header shows a search icon instead, so search stays one tap away.

import { expect, test } from '@playwright/test';
import { newProject, signIn } from './helpers.ts';

test('search opens from the header at desktop and phone widths, and the sidebar no longer repeats it', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}`);
	const palette = page.getByPlaceholder('Search or jump to a page');

	// The sidebar holds navigation only.
	await expect(page.locator('[data-sidebar="sidebar"]').first()).toBeVisible();
	await expect(page.locator('[data-sidebar="sidebar"]').getByText('Search', { exact: true })).toHaveCount(0);

	// Desktop: the wide button in the header.
	await page.getByRole('button', { name: /^Search/ }).click();
	await expect(palette).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(palette).toBeHidden();

	// Phone: the wide button is gone and the icon takes its place.
	await page.setViewportSize({ width: 390, height: 844 });
	await expect(page.getByRole('button', { name: /^Search.+/ })).toHaveCount(0);
	await page.getByRole('button', { name: 'Search', exact: true }).click();
	await expect(palette).toBeVisible();
});
