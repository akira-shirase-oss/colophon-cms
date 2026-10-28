// Which Colophon this is (gap audit, 2026-10-28): "About Colophon" in the account menu, on every
// screen and for every role, shows the server's version and build day from `GET /version` in
// plain words, keeps the commit and API versions in the technical details, and copies all of it
// (with the address, page and browser) for a problem report. If the server doesn't answer, it says
// so and offers to try again; it never spins forever.

import { expect, test } from '@playwright/test';
import { internalTerms, newProject, signIn, violations } from './helpers.ts';

test('About Colophon says which server this is, in plain words, and copies it for a problem report', async ({ page, context, request }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	const answer = await request.get('/version');
	expect(answer.status()).toBe(200);
	const server = (await answer.json()) as { version: string; commit: string; build_date: string; api: string[] };

	await signIn(page);
	await page.getByTestId('account-menu').click();
	await page.getByTestId('about-colophon').click();
	const about = page.getByTestId('about');
	await expect(about.getByRole('heading', { name: 'About Colophon' })).toBeVisible();
	await expect(about.getByTestId('about-version')).toHaveText(server.version);
	const day = /^\d{4}-\d{2}-\d{2}$/.test(server.build_date)
		? new Date(`${server.build_date}T00:00:00Z`).toLocaleDateString('en-US', { dateStyle: 'long', timeZone: 'UTC' })
		: 'Not recorded';
	await expect(about.getByTestId('about-built')).toHaveText(day);
	// The commit is a technical detail: closed until asked for.
	await expect(about.getByTestId('about-commit')).toBeHidden();
	await about.getByTestId('technical-details').locator('summary').click();
	await expect(about.getByTestId('about-commit')).toHaveText(server.commit);
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	await about.getByTestId('about-copy').click();
	await expect(page.getByText('Copied. Paste it into your message.')).toBeVisible();
	const copied = await page.evaluate(() => navigator.clipboard.readText());
	expect(copied).toContain(`Colophon ${server.version}`);
	expect(copied).toContain(`Commit: ${server.commit}`);
	expect(copied).toContain(`API versions: ${server.api.join(', ')}`);
	expect(copied).toContain(`Page: ${page.url()}`);
	await about.getByRole('button', { name: 'Close' }).click();
	await expect(about).toBeHidden();

	// Inside a project too, where the account menu sits in the sidebar's bar; and when the server
	// doesn't answer, it says so and tries again on request.
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}`);
	await page.route('**/version', (route) => route.abort());
	await page.getByTestId('account-menu').click();
	await page.getByTestId('about-colophon').click();
	await expect(about.getByTestId('problem')).toContainText("Couldn't load the version details", { timeout: 20_000 });
	await expect(about.getByTestId('about-copy')).toBeDisabled();
	await page.unroute('**/version');
	await about.getByRole('button', { name: 'Try again' }).click();
	await expect(about.getByTestId('about-version')).toHaveText(server.version);
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// On a phone, it fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await expect(about.getByTestId('about-copy')).toBeVisible();
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth)).toBeLessThanOrEqual(0);
});
