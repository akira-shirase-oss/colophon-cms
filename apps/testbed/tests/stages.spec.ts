// Named review stages through the testbed, in a real browser (D-104): replace the
// environment's stage list under its ETag, then a changeset publishes only once every
// stage approves — approving one stage leaves publish refused, approving the second
// lets it through.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function setup(page: Page): Promise<{ project: string }> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
		const created = await fetch('/server/api/v1/projects', { method: 'POST', headers: json, body: JSON.stringify({ slug, name: 'Stages testbed' }) });
		const project = ((await created.json()) as { data: { id: string } }).data.id;
		const body = { title: 'Staged note', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
		await fetch(`/server/api/v1/projects/${project}/environments/production/collections/articles/entries`, {
			method: 'POST',
			headers: { ...json, 'Idempotency-Key': crypto.randomUUID() },
			body: JSON.stringify(body)
		});
		return { project };
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('define two stages, gate a publish on both, then release it', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project } = await setup(page);
	await page.goto('/stages');
	await page.getByLabel('Project').selectOption(project);

	// No stages yet: the list is empty and its ETag already names it. The article checkbox
	// appearing says the page's first read finished entirely, so filling the list below
	// can't be clobbered by a read still in flight.
	await expect(page.getByTestId('stages')).toContainText('No stages');
	await expect(page.getByTestId('etag')).toHaveText(/sha256:/);
	await expect(page.getByLabel('Staged note', { exact: true })).toBeVisible();

	// Two stages, the second needing someone other than the submitter — here the submitter
	// has dev-owner approve it anyway (the constraint is that the *submitter's* approval
	// doesn't count, which a second member's does).
	await page.getByTestId('stages-json').fill(
		JSON.stringify([
			{ key: 'editorial', name: 'Editorial', different_reviewer: false },
			{ key: 'legal', name: 'Legal', different_reviewer: false }
		])
	);
	await page.getByTestId('apply-stages').click();
	await expect(page.getByTestId('message')).toHaveText('replaced');
	await expect(page.getByTestId('stages')).toContainText('editorial — Editorial');
	await expect(page.getByTestId('stages')).toContainText('legal — Legal');

	// Create and submit a changeset; each stage waits.
	await page.getByLabel('Changeset title').fill('Staged publish');
	await page.getByLabel('Staged note', { exact: true }).check();
	await page.getByRole('button', { name: 'Create changeset' }).click();
	await expect(page.getByTestId('state')).toHaveText('open');
	await page.getByRole('button', { name: 'Submit' }).click();
	await expect(page.getByTestId('state')).toHaveText('awaiting_review');
	await expect(page.getByTestId('stage-state')).toHaveCount(2);

	// A review that names no stage is refused; one that names an unknown stage is too.
	await page.getByTestId('review-stage').selectOption('');
	await page.getByTestId('approve').click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (stage_required)');

	// Approve stage one; publish still refuses because stage two waits.
	await page.getByTestId('review-stage').selectOption('editorial');
	await page.getByTestId('approve').click();
	await expect(page.getByTestId('stage-state').nth(0)).toHaveAttribute('data-state', 'approved');
	await expect(page.getByTestId('stage-state').nth(1)).toHaveAttribute('data-state', 'pending');
	await expect(page.getByTestId('state')).toHaveText('awaiting_review');
	await page.getByTestId('publish').click();
	await expect(page.getByTestId('message')).toHaveText('409 conflict (not_approved)');

	// Approve stage two; publish goes through.
	await page.getByTestId('review-stage').selectOption('legal');
	await page.getByTestId('approve').click();
	await expect(page.getByTestId('state')).toHaveText('approved');
	await page.getByTestId('publish').click();
	await expect(page.getByTestId('message')).toContainText('published (generation');
	await expect(page.getByTestId('state')).toHaveText('published');
});
