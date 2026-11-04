// GraphQL (DX.10, D-115) through the testbed, in a real browser, against the real
// server: `entries` runs the same canonical document `entries/query` echoes — the
// hashes match — a nested reference expands, `entry(id)` reads one draft, a filter
// the query language would refuse is a GraphQL error carrying its `code` and
// `pointer`, and a `mutation` is refused, writes staying REST's.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'GraphQL testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('entries runs the same document REST does, and one entry reads by id', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/graphql');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByRole('button', { name: 'Set up staff and posts' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');

	// The News post alone, its author expanded one level — and `applied` the canonical
	// document, hash-identical to the REST twin's: one compiled form ran both.
	await page.getByRole('button', { name: 'Query entries' }).click();
	await expect(page.getByTestId('titles')).toHaveText('Alpha news');
	await expect(page.getByTestId('author')).toHaveText('Ada');
	await expect(page.getByTestId('query-hash')).toContainText('sha256:');
	await expect(page.getByTestId('hash-match')).toHaveText('same document');

	await page.getByRole('button', { name: 'Read one entry' }).click();
	await expect(page.getByTestId('entry-title')).toHaveText('Alpha news');
});

test('a refused filter and a mutation are GraphQL errors, not leaks', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/graphql');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByRole('button', { name: 'Set up staff and posts' }).click();
	await expect(page.getByTestId('message')).toHaveText('set up');

	// `first: 0` compiles to `limit: 0`, which the document refuses — a GraphQL error
	// whose extensions carry the same code and pointer REST answers.
	await page.getByRole('button', { name: 'Ask for zero entries' }).click();
	await expect(page.getByTestId('refused')).toContainText('out_of_range');
	await expect(page.getByTestId('refused')).toContainText('/limit');

	await page.getByRole('button', { name: 'Send a mutation' }).click();
	await expect(page.getByTestId('mutation')).toContainText('mutation');
});
