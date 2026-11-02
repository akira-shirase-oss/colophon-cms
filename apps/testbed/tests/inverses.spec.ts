// Mirrored references (D-085) through the testbed, in a real browser, against the real server:
// a wrong inverse is refused where it is declared, and a post's writer link keeps the writer's
// own list — filled in on link, moved on save, emptied on delete.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Inverses testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('an inverse field mirrors every link change', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/inverses');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByRole('button', { name: 'Set up posts and writers' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');
	// `name` is a text field, not a reference back: refused where the mirror was declared.
	await expect(page.getByTestId('wrong-inverse')).toContainText('422 validation_failed (invalid_inverse at /fields/1/inverse)');

	await page.getByRole('button', { name: 'Link a post to Ada' }).click();
	await expect(page.getByTestId('message')).toHaveText('linked');
	const postId = await page.getByTestId('post-id').textContent();
	expect(postId).toMatch(/^[0-9a-f-]{36}$/);
	await expect(page.getByTestId('ada-posts')).toHaveText(postId ?? '');
	await expect(page.getByTestId('grace-posts')).toHaveText('(none)');

	await page.getByRole('button', { name: 'Move it to Grace' }).click();
	await expect(page.getByTestId('message')).toHaveText('moved');
	await expect(page.getByTestId('ada-posts')).toHaveText('(none)');
	await expect(page.getByTestId('grace-posts')).toHaveText(postId ?? '');

	await page.getByRole('button', { name: 'Delete it' }).click();
	await expect(page.getByTestId('message')).toHaveText('deleted');
	await expect(page.getByTestId('grace-posts')).toHaveText('(none)');
});
