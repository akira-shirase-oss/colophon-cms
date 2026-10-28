// "Your work", the home page after signing in: in the project someone last opened, their own
// drafts that aren't live, publish requests waiting for review (said for who can approve them),
// their own requests in progress, what is scheduled, and the last releases with their labels.
// Two people see their own work; plain words, axe, and a phone without sideways scrolling.

import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, member, newEntry, newProject, publishThroughApi, shot, signIn, violations } from './helpers.ts';

/** A publish request of `entries`, sent for review; its id and hash. */
async function sent(page: Page, project: string, title: string, entries: string[]): Promise<{ id: string; hash: string }> {
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title, entries }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const set = made.body?.['data'] as { id: string; hash: string };
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	return set;
}

test('two people each see their own work on the home page, on a desk and on a phone', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}`);
	await expect(page.getByTestId('first-run')).toBeVisible();

	const editor = await member(page, browser, project, 'editor');
	await expect(editor.getByTestId('first-run')).toBeVisible();
	await newEntry(editor, project, 'The editor’s draft');
	await sent(editor, project, 'Needs a look', [await newEntry(editor, project, 'Needs a look')]);

	await newEntry(page, project, 'The owner’s draft');
	await publishThroughApi(page, project, 'Already out', [await newEntry(page, project, 'Already out')]);
	const later = await sent(page, project, 'Tomorrow’s piece', [await newEntry(page, project, 'Tomorrow’s piece')]);
	expect((await api(page, 'POST', `${env(project)}/changesets/${later.id}/reviews`, { decision: 'approve' }, { 'If-Match': `"${later.hash}"` })).status).toBe(200);
	const tomorrow = new Date(Date.now() + 24 * 3600 * 1000).toISOString().slice(0, 10);
	const scheduled = await api(page, 'POST', `${env(project)}/changesets/${later.id}/schedule`, { local_time: `${tomorrow}T09:00`, time_zone: 'UTC' }, { 'If-Match': `"${later.hash}"` });
	expect(scheduled.status, JSON.stringify(scheduled.body)).toBe(201);

	// The owner: their drafts, what waits for their review, the schedule, the release.
	await page.goto('/studio/');
	await expect(page.getByRole('heading', { level: 1, name: 'Your work' })).toBeVisible();
	await expect(page.getByTestId('signed-in')).toBeVisible();
	const drafts = page.getByTestId('my-drafts');
	await expect(drafts).toContainText('The owner’s draft');
	await expect(drafts).not.toContainText('The editor’s draft');
	await expect(drafts).not.toContainText('Already out');
	await expect(page.getByRole('heading', { name: 'Waiting for your review' })).toBeVisible();
	await expect(page.getByTestId('waiting-review')).toContainText('Needs a look');
	await expect(page.getByTestId('my-schedules-list')).toContainText('Tomorrow’s piece');
	await expect(page.getByTestId('my-releases-list')).toContainText('Release 1');
	await expect(page.getByTestId('my-releases-list')).toContainText('Published in CMS');
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await shot(page, 'my-work-owner');
	await page.getByTestId('waiting-review').getByRole('link', { name: /Needs a look/ }).click();
	await expect(page.getByTestId('changeset-status')).toHaveAttribute('data-state', 'awaiting_review');

	// The editor: their own drafts, and the request they sent, waiting for a publisher.
	await editor.goto('/studio/');
	const theirs = editor.getByTestId('my-drafts');
	await expect(theirs).toContainText('The editor’s draft');
	await expect(theirs).not.toContainText('The owner’s draft');
	await expect(editor.getByRole('heading', { name: 'Waiting for review: a publisher approves these' })).toBeVisible();
	await expect(editor.getByTestId('waiting-review')).toContainText('Needs a look');
	expect(await violations(editor)).toEqual([]);
	expect(await internalTerms(editor)).toEqual([]);

	// On a phone (D-059, rule 7): the same page, without sideways scrolling.
	const who = await api(editor, 'GET', '/me');
	const phone = await (await browser.newContext({ viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
	await phone.context().addCookies(await editor.context().cookies());
	await phone.addInitScript(([name, value]) => localStorage.setItem(name, value), [`colophon:last-project:${who.body?.['data'].id}`, project] as const);
	await phone.goto('/studio/');
	await expect(phone.getByTestId('my-drafts')).toContainText('The editor’s draft');
	await phone.waitForLoadState('networkidle');
	expect(await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the home page scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(phone)).toEqual([]);
	await shot(phone, 'my-work-phone');
	await phone.context().close();
	await editor.context().close();
});
