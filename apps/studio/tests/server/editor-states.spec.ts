// The entry editor in each state an editor meets it in: a new draft, live, live with changes, in
// review and taken down. Each is checked with axe in light and dark at a desktop (1440×900) and a
// phone (390×844) width, with exactly one primary action where there is something to do and none
// where there isn't; with STUDIO_SHOTS_DIR set, each is also photographed there as
// `editor-<state>-<width>-<scheme>.png` (the before-and-after pairs in docs/screens).

import { mkdirSync } from 'node:fs';
import { type Browser, expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

const SHOTS = process.env['STUDIO_SHOTS_DIR'];

function body(...paragraphs: string[]) {
	return {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: paragraphs.map((text) => ({ type: 'paragraph', content: [{ type: 'text', text }] })) }
	};
}

const article = (title: string, ...paragraphs: string[]) => ({ title, body: body(...paragraphs) });

/** Saves `title` as the entry's new draft through the API, as another tab would. */
async function retitle(page: Page, project: string, entry: string, title: string) {
	const path = `${env(project)}/collections/articles/entries/${entry}`;
	const current = await page.evaluate(async (path) => {
		const response = await fetch(path);
		return { etag: response.headers.get('etag') ?? '', content: ((await response.json()) as { data: { content: Record<string, unknown> } }).data.content };
	}, path);
	const saved = await api(page, 'PUT', path, { ...current.content, title }, { 'If-Match': current.etag, 'Idempotency-Key': key() });
	expect(saved.status, JSON.stringify(saved.body)).toBe(200);
}

interface World {
	project: string;
	states: { state: string; entry: string; badge: string; primary: string | null }[];
}

/** A small magazine with an entry in every state. */
async function world(page: Page): Promise<World> {
	const project = await newProject(page, 'Harbour Gazette');
	const draft = await newEntry(page, project, '', 'articles', article(
		'Ferry timetable for the winter season',
		'From the first of December the morning ferry leaves at 7:40, twenty minutes later than in summer, and the last crossing back is at 18:10.',
		'The Sunday service keeps its two sailings. Bikes still travel free; cars need a booking at least a day ahead.'
	));
	const live = await newEntry(page, project, '', 'articles', article(
		'Spring planting guide for small gardens',
		'Start with the soil. A balcony box needs fresh compost every spring, and a raised bed needs a forkful of manure dug in before the frosts end.',
		'Peas, broad beans and radishes go in first; tomatoes wait until the nights are warm.'
	));
	const changed = await newEntry(page, project, '', 'articles', article(
		'Storm week: how the coast road reopened',
		'The coast road closed on Monday night, when the tide came over the sea wall at Penmarch and took forty metres of the verge with it.',
		'By Thursday the council crews had a single lane open. This is how they did it, and what the village did in the meantime.'
	));
	const review = await newEntry(page, project, '', 'articles', article(
		'Interview: the last letterpress printer in town',
		'Marie Kerjean has set type by hand in the same workshop on Rue du Port for thirty-one years.',
		'She still prints the parish newsletter, wedding cards and, every summer, the regatta programme.'
	));
	const takenDown = await newEntry(page, project, '', 'articles', article(
		'The Saturday market moves to the quay',
		'While the square is repaved, the Saturday market sets up along the quay, between the fish auction and the lifeboat station.'
	));
	await publishThroughApi(page, project, 'Spring stories', [live, changed, takenDown]);
	await publishThroughApi(page, project, 'Market back in the square', [], [takenDown]);
	await retitle(page, project, changed, 'Storm week: how the coast road reopened in four days');
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title: 'The printer interview', entries: [review] }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const set = made.body?.['data'] as { id: string; hash: string };
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
	return {
		project,
		states: [
			{ state: 'new-draft', entry: draft, badge: 'draft', primary: 'Publish' },
			{ state: 'live', entry: live, badge: 'live', primary: null },
			{ state: 'live-with-changes', entry: changed, badge: 'changed', primary: 'Publish changes' },
			{ state: 'in-review', entry: review, badge: 'review', primary: 'Review the request' },
			{ state: 'taken-down', entry: takenDown, badge: 'taken_down', primary: 'Publish again' }
		]
	};
}

const SIZES = [
	{ width: 1440, height: 900, phone: false },
	{ width: 390, height: 844, phone: true }
] as const;

/** The primary buttons a person can see on the page (dialogs aside): the accent's one job. */
async function primaries(page: Page): Promise<string[]> {
	return page
		.locator('[data-slot="button"][data-variant="default"]:visible')
		.evaluateAll((buttons) => buttons.filter((button) => button.closest('[role="dialog"]') === null).map((button) => button.textContent?.trim() ?? ''));
}

async function viewerAt(browser: Browser, size: (typeof SIZES)[number]) {
	const context = await browser.newContext({
		viewport: { width: size.width, height: size.height },
		...(size.phone ? { isMobile: true, hasTouch: true } : {}),
		reducedMotion: 'reduce'
	});
	const page = await context.newPage();
	await signIn(page);
	return page;
}

test('the editor in every state: one primary where there is something to do, axe in light and dark, on a desktop and a phone', async ({ page, browser }) => {
	test.setTimeout(240_000);
	await signIn(page);
	const made = await world(page);
	if (SHOTS !== undefined && SHOTS !== '') mkdirSync(SHOTS, { recursive: true });

	for (const size of SIZES) {
		const viewer = await viewerAt(browser, size);
		for (const { state, entry, badge, primary } of made.states) {
			await viewer.goto(`/studio/projects/${made.project}/articles/${entry}`);
			await expect(viewer.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
			await viewer.waitForLoadState('networkidle');
			// Soft, so one run reports every state that has a problem.
			await expect.soft(viewer.getByTestId('entry-state'), `${state}, ${size.width}`).toHaveAttribute('data-state', badge);
			// The role read has arrived: before it no primary shows, so an expected "none" proves nothing.
			await expect(viewer.getByTestId('publish-entry')).not.toContainText('Checking what your role may do');
			for (const scheme of ['light', 'dark'] as const) {
				await viewer.emulateMedia({ colorScheme: scheme });
				await expect(viewer.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
				expect.soft(await violations(viewer), `${state}, ${size.width}, ${scheme}`).toEqual([]);
				expect.soft(await primaries(viewer), `${state}, ${size.width}, ${scheme}`).toEqual(primary === null ? [] : [primary]);
				if (SHOTS !== undefined && SHOTS !== '') await viewer.screenshot({ path: `${SHOTS}/editor-${state}-${size.width}-${scheme}.png` });
			}
			if (size.phone) {
				expect.soft(await viewer.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), `${state}: no sideways scroll`).toBeLessThanOrEqual(0);
			}
		}
		await viewer.context().close();
	}
});
