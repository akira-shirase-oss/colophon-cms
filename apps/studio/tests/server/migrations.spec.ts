// A content-schema migration end to end in the Studio (step M.5c; D-100, D-101): an owner
// narrows a field's allowed form while an entry doesn't fit, the save is refused naming it,
// and the refusal's "plan a migration" makes the plan. Its page names the entry in the way,
// a person resolves it with a document that fits, and Approve then Apply — each on the hash
// on screen — starts the run the server's worker drives to done.

import { expect, test } from '@playwright/test';
import { applyField, editField, newEntry, newProject, saveBuilder, signIn, violations } from './helpers.ts';

const BODY = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };

test('a refused type change becomes a migration the Studio drives to done', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	// A draft whose title won't fit the pattern the type is about to gain.
	const draft = { title: 'lowercase launch', body: BODY };
	await newEntry(page, project, 'lowercase launch', 'articles', draft);

	// Narrow the title's allowed form in Content types: the stored title doesn't fit, so the
	// save is refused naming the entry, and offers to plan the migration instead.
	await page.goto(`/studio/projects/${project}/settings/types/articles`);
	await (await editField(page, 'title', 'Advanced')).getByTestId('field-pattern').fill('[A-Z].*');
	await applyField(page);
	await saveBuilder(page);
	const refusal = page.getByTestId('type-refusal');
	await expect(refusal).toBeVisible();
	await expect(refusal.getByRole('link', { name: 'lowercase launch' })).toBeVisible();
	await expect(page.getByTestId('plan-migration')).toBeVisible();

	// Planning opens the plan: proposed, the dry run naming the entry in the way.
	await page.getByTestId('plan-migration').click();
	await expect(page).toHaveURL(/\/studio\/projects\/[^/]+\/migrations\/[0-9a-f-]+/);
	await expect(page.getByRole('heading', { name: 'Articles migration' })).toBeVisible();
	await expect(page.getByTestId('migration-status')).toHaveAttribute('data-state', 'proposed');
	const items = page.getByTestId('migration-items');
	await expect(items.getByRole('link', { name: 'lowercase launch' })).toBeVisible();
	await expect(items.getByTestId('item-outcome')).toHaveAttribute('data-outcome', 'needs_input');
	expect(await violations(page)).toEqual([]);

	// Resolve it: the dialog starts from the entry's document, and bad JSON is checked in the
	// page before anything is sent.
	await page.getByTestId('resolve-item').click();
	const resolution = page.getByTestId('resolution');
	await expect(resolution).toHaveValue(/lowercase launch/);
	await resolution.fill('{ not json');
	await page.getByTestId('resolve-submit').click();
	await expect(page.getByTestId('resolution-error')).toContainText("isn't JSON");
	// A document the target accepts resolves the item.
	await resolution.fill(JSON.stringify({ ...draft, title: 'Lowercase launch' }, null, 2));
	await page.getByTestId('resolve-submit').click();
	await expect(items.getByTestId('item-outcome')).toHaveAttribute('data-outcome', 'resolved');

	// Approve the plan shown, then apply it: a worker takes the run from there.
	await page.getByRole('button', { name: 'Approve plan' }).click();
	await expect(page.getByTestId('migration-status')).toHaveAttribute('data-state', 'approved');
	await page.getByRole('button', { name: 'Apply — start the migration' }).click();
	// Accepted, not done (D-101): the page follows the run until the worker finishes it.
	await expect(page.getByTestId('migration-status')).toHaveAttribute('data-state', 'done', { timeout: 30_000 });
	await expect(items.getByTestId('item-outcome')).toHaveAttribute('data-outcome', 'applied');
	expect(await violations(page)).toEqual([]);

	// The sidebar's Migrations lists it, done.
	await page.getByTestId('migrations-link').click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${project}/migrations$`));
	await expect(page.getByTestId('migration-state')).toHaveAttribute('data-state', 'done');
});
