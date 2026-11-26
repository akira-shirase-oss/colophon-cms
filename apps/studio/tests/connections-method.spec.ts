// Settings → Website connections names the HTTP method Colophon uses for each connection, beside
// its address (against the mocked API): a site check reads the page with GET; a site rebuild and
// a cache clear are sent with POST (the worker's own calls: `look` GETs, `post` POSTs). The badge
// reads as a sentence to a screen reader, the add form's kinds say the same, and at phone width
// the address truncates beside a whole badge instead of pushing the page sideways.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { PROJECT, serve, SETTINGS_URL, world } from './site-preview-world.ts';

const AT = '2026-11-17T12:00:00Z';
const LONG = `https://hooks.example.com/${'a-long-path/'.repeat(12)}rebuild`;
const CONNECTIONS = [
	{ id: 'd-1', kind: 'webhook', name: 'Site build', url: LONG, enabled: true, location: null, created_at: AT },
	{ id: 'd-2', kind: 'cdn', name: 'Edge cache', url: 'https://cdn.example.com/purge', enabled: true, location: null, created_at: AT },
	{ id: 'd-3', kind: 'probe', name: 'Home page', url: 'https://www.example.com/', enabled: true, location: 'eu-west', created_at: AT }
];

async function open(page: Page, baseURL: string | undefined) {
	await serve(page, world(), new URL(baseURL ?? '').origin);
	// A page route answers before the context's: this spec's connections, everything else the world's.
	await page.route(`**/api/v1/projects/${PROJECT}/environments/production/destinations`, (route) => route.fulfill({ json: { data: CONNECTIONS } }));
	await page.goto(SETTINGS_URL);
	await expect(page.getByTestId('destinations').getByRole('listitem')).toHaveCount(3);
}

test('each connection row names the method Colophon uses, as a badge and as a sentence', async ({ page, baseURL }) => {
	await open(page, baseURL);
	const rows = page.getByTestId('destinations').getByRole('listitem');
	for (const [index, method, sentence] of [
		[0, 'POST', 'Colophon sends a POST to this address'],
		[1, 'POST', 'Colophon sends a POST to this address'],
		[2, 'GET', 'Colophon reads this address with GET']
	] as const) {
		const badge = rows.nth(index).getByTestId('connection-method');
		await expect(badge).toHaveAttribute('data-method', method);
		// What is seen is the method alone; what is read out is the sentence.
		await expect(badge.locator('[aria-hidden="true"]')).toHaveText(method);
		await expect(badge.locator('.sr-only')).toHaveText(`${sentence}:`);
		await expect(badge).toHaveAttribute('title', sentence);
	}
	const results = await new AxeBuilder({ page }).analyze();
	expect(results.violations.map((violation) => violation.id)).toEqual([]);
});

test('the add form says the method beside each kind', async ({ page, baseURL }) => {
	await open(page, baseURL);
	await page.getByTestId('add-connection').click();
	const dialog = page.getByRole('dialog', { name: 'Add a website connection' });
	await expect(dialog.getByTestId('kind-method').locator('[aria-hidden="true"]')).toHaveText(['POST', 'POST', 'GET']);
	// The kinds are still found by their names.
	await expect(dialog.getByRole('radio', { name: /Site check/ })).toBeVisible();
	await expect(dialog.getByRole('radio', { name: /Colophon reads this address with GET/ })).toHaveCount(1);
});

test('at phone width the badge stays whole, the address truncates and the page does not scroll sideways', async ({ page, baseURL }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	await open(page, baseURL);
	const row = page.getByTestId('destinations').getByRole('listitem').first();
	const badge = await row.getByTestId('connection-method').boundingBox();
	const address = await row.locator('code').boundingBox();
	expect(badge).not.toBeNull();
	expect(address).not.toBeNull();
	// On one line, the badge before the address, and both inside the screen.
	expect(Math.abs((badge?.y ?? 0) + (badge?.height ?? 0) / 2 - ((address?.y ?? 0) + (address?.height ?? 0) / 2))).toBeLessThan(6);
	expect((badge?.x ?? 0) + (badge?.width ?? 0)).toBeLessThanOrEqual(address?.x ?? 0);
	expect((address?.x ?? 0) + (address?.width ?? 0)).toBeLessThanOrEqual(390);
	expect(await page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth)).toBe(true);
});
