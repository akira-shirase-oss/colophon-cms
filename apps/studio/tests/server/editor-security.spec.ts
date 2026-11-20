// What a hostile or careless value can do in the entry editor's panel (D-184): nothing. Titles,
// comments, alt text and links carrying markup, script URLs, a right-to-left override or
// zero-width characters are shown as text, never run; links the format doesn't allow are refused
// by the server; and "Copy public link" is offered only when anyone may read the live version —
// a member-only entry's address isn't a public link.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, openDetails, publishThroughApi, signIn } from './helpers.ts';

const MARKUP = '<img src=x onerror="window.__pwned=1">';
const SCRIPT = '<script>window.__pwned=1</script>';
const RTL = 'Tide‮selbat';
const ZERO_WIDTH = 'Ti​de‍ tab﻿les';

function doc(...content: object[]) {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content } };
}
const paragraph = (text: string, marks?: object[]) => ({ type: 'paragraph', content: [{ type: 'text', text, ...(marks ? { marks } : {}) }] });

/** Fails the test if any script the page didn't ship ran, or a dialog opened. */
async function guard(page: Page) {
	page.on('dialog', (dialog) => {
		throw new Error(`a dialog opened: ${dialog.message()}`);
	});
	await page.addInitScript(() => {
		(window as unknown as { __pwned?: number }).__pwned = 0;
	});
}
async function untouched(page: Page) {
	expect(await page.evaluate(() => (window as unknown as { __pwned?: number }).__pwned ?? 0)).toBe(0);
	expect(await page.locator('img[src="x"]').count()).toBe(0);
	expect(await page.locator('script:not([src]):not([type])').evaluateAll((scripts) => scripts.filter((one) => one.textContent?.includes('__pwned')).length)).toBe(0);
}

test('markup, script, RTL and zero-width titles are text everywhere the panel shows them', async ({ page }) => {
	await guard(page);
	await signIn(page);
	const project = await newProject(page);
	for (const title of [MARKUP, SCRIPT, RTL, ZERO_WIDTH, `${MARKUP} ${'x'.repeat(120 - MARKUP.length - 1)}`]) {
		const entry = await newEntry(page, project, '', 'articles', { title, body: doc(paragraph(title)) });
		const generation = await publishThroughApi(page, project, title.slice(0, 200), [entry]);
		await page.goto(`/studio/projects/${project}/articles/${entry}`);
		const heading = page.getByRole('heading', { level: 1 }).getByRole('textbox', { name: 'Title' });
		await expect(heading).toHaveValue(title);
		await expect(page.getByRole('navigation', { name: 'breadcrumb' })).toContainText(title.slice(0, 20));
		// The confirmation and the toasts name it too.
		await page.getByTestId('more-menu').click();
		await page.getByTestId('unpublish-button').click();
		await expect(page.getByRole('alertdialog').getByRole('heading')).toContainText(title.slice(0, 20));
		await page.getByRole('alertdialog').getByRole('button', { name: 'Unpublish' }).click();
		await expect(page.locator('[data-sonner-toast]').filter({ hasText: `Unpublished in Release ${generation + 1}` })).toBeVisible();
		await page.getByRole('tab', { name: /History/ }).click();
		await expect(page.getByTestId('history-row').first()).toBeVisible();
		await untouched(page);
	}
});

test('a 10,000-character title is refused by the server with the limit, and the editor says so', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const refused = await api(page, 'POST', `${env(project)}/collections/articles/entries`, { title: 'x'.repeat(10_000), body: doc(paragraph('x')) }, { 'Idempotency-Key': key() });
	expect(refused.status).toBe(422);
	const entry = await newEntry(page, project, 'Short');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	const heading = page.getByRole('heading', { level: 1 }).getByRole('textbox', { name: 'Title' });
	await heading.fill('x'.repeat(10_000));
	// The field stops at its limit rather than sending what the server would refuse.
	expect([...(await heading.inputValue())].length).toBeLessThanOrEqual(200);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
});

test('script and data links are refused by the server; an allowed link is rendered safely', async ({ page }) => {
	await guard(page);
	await signIn(page);
	const project = await newProject(page);
	for (const href of ['javascript:window.__pwned=1', 'JaVaScRiPt:alert(1)', 'data:text/html,<script>alert(1)</script>', '//evil.example', '/\t/evil.example', 'java\u0000script:alert(1)']) {
		const made = await api(page, 'POST', `${env(project)}/collections/articles/entries`, { title: 'Links', body: doc(paragraph('click', [{ type: 'link', attrs: { href } }])) }, { 'Idempotency-Key': key() });
		expect(made.status, `${JSON.stringify(href)}: ${JSON.stringify(made.body)}`).toBe(422);
	}
	const entry = await newEntry(page, project, '', 'articles', { title: 'Links', body: doc(paragraph('the harbour', [{ type: 'link', attrs: { href: 'https://example.org/harbour' } }])) });
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	const link = page.getByRole('textbox', { name: 'Body' }).getByRole('link', { name: 'the harbour' });
	await expect(link).toHaveAttribute('href', 'https://example.org/harbour');
	await untouched(page);
});

test('a comment and an image’s alt text carrying markup are shown as text', async ({ page }) => {
	await guard(page);
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, '', 'articles', { title: 'Comments', body: doc(paragraph('Some text.')) });
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await page.getByTestId('comments-button').click();
	const drawer = page.getByTestId('comments-drawer');
	await drawer.getByTestId('comment-input').fill(`${MARKUP} ${SCRIPT} ${RTL}`);
	await drawer.getByTestId('comment-post').click();
	await expect(drawer.getByTestId('comment')).toContainText(MARKUP);
	await page.reload();
	await page.getByTestId('comments-button').click();
	await expect(page.getByTestId('comments-drawer').getByTestId('comment')).toContainText(SCRIPT);
	await untouched(page);
});

test('Copy public link is offered only when anyone may read the live version', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const role = await api(page, 'POST', `/api/v1/projects/${project}/audience/roles`, { name: 'member', self_assign: true });
	expect(role.status, JSON.stringify(role.body)).toBe(201);
	const cases = [
		{ gate: 'public', copy: true, words: null },
		{ gate: 'authenticated', copy: false, words: 'Only readers signed in on your site can read it.' },
		{ gate: 'role:member', copy: false, words: 'Only readers with the “member” role can read it.' }
	];
	for (const { gate, copy, words } of cases) {
		const made = await api(page, 'POST', `${env(project)}/collections/articles/entries?visibility=${gate}`, { title: `Gate ${gate}`, body: doc(paragraph('text')) }, { 'Idempotency-Key': key() });
		expect(made.status, JSON.stringify(made.body)).toBe(201);
		const entry = made.body?.['data'].id as string;
		await publishThroughApi(page, project, `Gate ${gate}`, [entry]);
		await page.goto(`/studio/projects/${project}/articles/${entry}`);
		await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
		await page.getByTestId('more-menu').click();
		await expect(page.getByTestId('copy-public-link'), gate).toHaveCount(copy ? 1 : 0);
		await page.keyboard.press('Escape');
		await openDetails(page);
		await expect(page.getByTestId('copy-public-link-details'), gate).toHaveCount(copy ? 1 : 0);
		if (words !== null) await expect(page.getByTestId('reader-gate')).toHaveText(words);
		// Anyone not signed in gets nothing for a gated one: the link would have been a dead end.
		const status = await page.evaluate(async (path) => (await fetch(path, { credentials: 'omit' })).status, `/content/v1/${project}/production/articles/${entry}`);
		expect(status, gate).toBe(copy ? 200 : 404);
	}
});
