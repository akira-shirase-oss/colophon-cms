// Locales (D-087) through the testbed, in a real browser, against the real server: an English
// note grows a French sibling through the localizations route, the group lists both members,
// a second French version is refused, and the list keeps only the French row. Then the
// environment's languages: set with their fallbacks (a save on the old version refused), posts
// published in a few of them, and the anonymous delivery API read with ?locale= — each post's
// best version, the fallbacks marked, an entry answered by its sibling with its other versions
// named, and a language with nothing published refused as not_in_locale.

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

test('the delivery API answers in a language, falling back as the list says', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const projectId = await newProject(page);
	await page.goto('/locales');
	await page.getByLabel('Project').selectOption(projectId);

	// The languages, saved on top of the list's ETag; the same save on the old one is refused.
	await page.getByRole('button', { name: 'Set the languages' }).click();
	await expect(page.getByTestId('languages')).toHaveText('en (default); pt → en; pt-BR → pt, en; fr → en — version 1');
	await page.getByRole('button', { name: 'Save them again on the old version' }).click();
	await expect(page.getByTestId('stale')).toHaveText('409 conflict (stale_locales)');

	await page.getByRole('button', { name: 'Publish posts in a few languages' }).click();
	await expect(page.getByTestId('published')).toHaveText(/^published as generation \d+$/);
	await expect(page.getByTestId('message')).toHaveCount(0);

	// One row per post, each its best version for pt-BR → pt → en; the fallbacks are marked.
	await page.getByRole('button', { name: 'Read the posts in pt-BR' }).click();
	await expect(page.getByTestId('content-language')).toHaveText('pt-BR');
	const rows = page.getByTestId('published-row');
	await expect(rows).toHaveCount(3);
	const oi = rows.filter({ hasText: 'Oi:' });
	await expect(oi).toContainText('pt-BR → pt-BR');
	await expect(oi.getByTestId('fallback-badge')).toHaveCount(0);
	const bomDia = rows.filter({ hasText: 'Bom dia:' });
	await expect(bomDia).toContainText('pt-BR → pt');
	await expect(bomDia.getByTestId('fallback-badge')).toBeVisible();
	const english = rows.filter({ hasText: 'Only in English:' });
	await expect(english).toContainText('pt-BR → en');
	await expect(english.getByTestId('fallback-badge')).toBeVisible();
	// No English or Portuguese duplicate of a post that has a better version.
	await expect(rows.filter({ hasText: 'Hello:' })).toHaveCount(0);
	await expect(rows.filter({ hasText: 'Olá:' })).toHaveCount(0);

	// The English post's id, asked for in pt-BR: its Portuguese sibling answers, naming the id
	// that was asked for, and the English version is its alternate.
	const morningEn = await page.getByTestId('morning-en').textContent();
	const morningPt = await page.getByTestId('morning-pt').textContent();
	await page.getByRole('button', { name: 'Read Good morning in pt-BR' }).click();
	await expect(page.getByTestId('entry-read')).toHaveText(`${morningEn} → ${morningPt} (pt, fallback)`);
	await expect(page.getByTestId('alternates')).toHaveText('en good-morning');

	// French with no fallback: nothing published in it, and the refusal names what is.
	await page.getByRole('button', { name: 'Read Only in English in French, no fallback' }).click();
	await expect(page.getByTestId('missing')).toHaveText('404 not_found (not_in_locale); published in: en');
});
