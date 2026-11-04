// API tokens and the audit log, through the testbed (step R.4, D-063): make a read-only token and
// see its secret once, read articles with the secret alone, be refused a token that manages
// members, revoke the token and be refused with it, and read the changes back newest first.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Tokens testbed' })
		});
		if (!response.ok) throw new Error(`setup: project: HTTP ${response.status} ${await response.text()}`);
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

test('make a token, use its secret, refuse a people-only token, revoke it, read the log', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/tokens');
	await page.getByLabel('Project').selectOption(projectId);
	await expect(page.getByTestId('tokens').locator('li')).toHaveCount(0);

	await page.getByRole('button', { name: 'Create' }).click();
	await expect(page.getByTestId('message')).toHaveText('created read-only token');
	await expect(page.getByTestId('secret')).toHaveText(/^colophon_api_[A-Za-z0-9_-]{43}$/);
	const listed = page.getByTestId('tokens').locator('li');
	await expect(listed).toHaveCount(1);
	await expect(listed.first()).toHaveAttribute('data-state', 'active');
	await expect(listed.first()).not.toContainText('secret listed');

	await page.getByRole('button', { name: 'Use it' }).click();
	await expect(page.getByTestId('used')).toHaveText('200 read articles');

	await page.getByLabel('Kind').selectOption('custom');
	await page.getByLabel('Permissions for custom (JSON)').fill('[{"action": "members.manage", "subject": "*"}]');
	await page.getByRole('button', { name: 'Create' }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (not_for_tokens)');

	await page.getByRole('button', { name: 'Revoke it' }).click();
	await expect(page.getByTestId('message')).toHaveText('revoked (revoked)');
	await page.getByRole('button', { name: 'Use it' }).click();
	await expect(page.getByTestId('used')).toHaveText('401 unauthorized');

	const log = page.getByTestId('audit').locator('li');
	await expect(log.nth(0)).toHaveAttribute('data-action', 'token.revoked');
	await expect(log.nth(1)).toHaveAttribute('data-action', 'token.created');
	await expect(log.nth(0)).toContainText('by a person');
});

test('a token created with provider ceilings shows them in the list', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/tokens');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByLabel('Provider tokens per day (optional)').fill('4000');
	await page.getByLabel('Provider cost per day, micros (optional)').fill('2500000');
	await page.getByRole('button', { name: 'Create' }).click();
	await expect(page.getByTestId('message')).toHaveText('created read-only token');

	// The ceilings came back on the token's view, next to its name.
	const limits = page.getByTestId('tokens').locator('li').first().getByTestId('token-limits');
	await expect(limits).toContainText('tokens_per_day=4000');
	await expect(limits).toContainText('cost_per_day_micros=2500000');
});

test('a token over its requests-per-minute ceiling is refused, and the log records it once', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/tokens');
	await page.getByLabel('Project').selectOption(projectId);

	await page.getByLabel('Requests per minute (optional)').fill('1');
	await page.getByRole('button', { name: 'Create' }).click();
	await expect(page.getByTestId('message')).toHaveText('created read-only token');

	// One request a minute: the dozen reads get one in, then the quota refuses every next one.
	await page.getByRole('button', { name: 'Keep using it' }).click();
	await expect(page.getByTestId('used')).toHaveText('1×200 then 429 rate_limited (rate_limited)');

	// One throttling episode, one event: the repeated refusals inside it are not logged again.
	const log = page.getByTestId('audit').locator('li');
	await expect(log.nth(0)).toHaveAttribute('data-action', 'token.rate_limited');
	await expect(log.nth(0)).toContainText('by a token');
	await expect(log.nth(1)).toHaveAttribute('data-action', 'token.created');
	await expect(log.filter({ hasText: 'token.rate_limited' })).toHaveCount(1);
});
