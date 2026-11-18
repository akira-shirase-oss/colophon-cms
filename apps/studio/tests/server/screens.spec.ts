// The Studio's main screens with real work in them: home, a project's overview, the articles
// list, the editor, a publish request and a receipt. Each is checked with axe in light and dark,
// at a desktop (1440×900) and a phone (390×844) width, and has at most one primary button
// (D-182); with STUDIO_SHOTS_DIR set, each is also photographed there as
// `<screen>-<width>-<scheme>.png` (docs/screens).
//
// The project holds every state the status badge has to tell apart: live, live with changes,
// draft, taken down, a request waiting for review and one scheduled for tomorrow, and a title of
// 120 characters to show what a long one does to each layout.

import { mkdirSync } from 'node:fs';
import { type Browser, expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

const SHOTS = process.env['STUDIO_SHOTS_DIR'];

/** 120 characters, as the audit's long-title fixture asks. */
const LONG_TITLE = 'What we learned rebuilding the harbour library after the winter flood, told by the volunteers who carried every book home';

function body(...paragraphs: string[]) {
	return {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: paragraphs.map((text) => ({ type: 'paragraph', content: [{ type: 'text', text }] })) }
	};
}

/** Saves `title` as the entry's new draft through the API, as another tab would. */
async function retitle(page: Page, project: string, entry: string, title: string) {
	const path = `${env(project)}/collections/articles/entries/${entry}`;
	const status = await page.evaluate(
		async ({ path, title }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const current = await fetch(path);
			const draft = ((await current.json()) as { data: { content: Record<string, unknown> } }).data;
			const saved = await fetch(path, {
				method: 'PUT',
				headers: {
					'Content-Type': 'application/json',
					'X-CSRF-Token': csrf,
					'If-Match': current.headers.get('etag') ?? '',
					'Idempotency-Key': crypto.randomUUID().replaceAll('-', '')
				},
				body: JSON.stringify({ ...draft.content, title })
			});
			return saved.status;
		},
		{ path, title }
	);
	expect(status).toBe(200);
}

/** A publish request for `entries`, sent for review (and approved, when `approve`); its id and hash. */
async function request(page: Page, project: string, title: string, entries: string[], approve = false) {
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title, entries }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const set = made.body?.['data'] as { id: string; hash: string };
	const ifMatch = { 'If-Match': `"${set.hash}"` };
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, ifMatch)).status).toBe(200);
	if (approve) expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/reviews`, { decision: 'approve' }, ifMatch)).status).toBe(200);
	return set;
}

interface World {
	project: string;
	editing: string;
	waiting: string;
}

/** A small magazine's project, as an editor would have it after a few weeks. */
async function world(page: Page): Promise<World> {
	const project = await newProject(page, 'Harbour Gazette');
	const storm = await newEntry(page, project, 'Storm week: how the coast road reopened', 'articles', {
		title: 'Storm week: how the coast road reopened',
		body: body(
			'The coast road closed on Monday night, when the tide came over the sea wall at Penmarch and took forty metres of the verge with it.',
			'By Thursday the council crews had a single lane open. This is how they did it, and what the village did in the meantime.'
		)
	});
	const planting = await newEntry(page, project, 'Spring planting guide for small gardens');
	const market = await newEntry(page, project, 'The Saturday market moves to the quay');
	await publishThroughApi(page, project, 'Storm week and spring planting', [storm, planting, market]);
	await publishThroughApi(page, project, 'Market moves back', [], [market]);
	await retitle(page, project, storm, 'Storm week: how the coast road reopened in four days');
	const interview = await newEntry(page, project, 'Interview: the last letterpress printer in town');
	const library = await newEntry(page, project, LONG_TITLE);
	await newEntry(page, project, 'Ferry timetable for the winter season');
	const waiting = await request(page, project, 'The printer interview', [interview]);
	const scheduled = await request(page, project, 'Harbour library story', [library], true);
	const tomorrow = new Date(Date.now() + 26 * 3600_000).toISOString().slice(0, 16);
	const at = await api(page, 'POST', `${env(project)}/changesets/${scheduled.id}/schedule`, { local_time: `${tomorrow}:00`, time_zone: 'UTC' }, { 'If-Match': `"${scheduled.hash}"` });
	expect(at.status, JSON.stringify(at.body)).toBe(201);
	return { project, editing: storm, waiting: waiting.id };
}

const SCREENS = (w: World) =>
	[
		{ name: 'home', path: '/studio/' },
		{ name: 'overview', path: `/studio/projects/${w.project}` },
		{ name: 'list', path: `/studio/projects/${w.project}/articles` },
		{ name: 'editor', path: `/studio/projects/${w.project}/articles/${w.editing}` },
		{ name: 'request', path: `/studio/projects/${w.project}/changesets/${w.waiting}` },
		{ name: 'receipt', path: `/studio/projects/${w.project}/releases/2` }
	] as const;

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

async function phoneOrDesktop(browser: Browser, size: (typeof SIZES)[number]) {
	const context = await browser.newContext({
		viewport: { width: size.width, height: size.height },
		...(size.phone ? { isMobile: true, hasTouch: true } : {}),
		reducedMotion: 'reduce'
	});
	const page = await context.newPage();
	await signIn(page);
	return page;
}

test('the main screens read in light and dark, on a desktop and a phone', async ({ page, browser }) => {
	test.setTimeout(180_000);
	await signIn(page);
	const made = await world(page);
	if (SHOTS !== undefined && SHOTS !== '') mkdirSync(SHOTS, { recursive: true });

	for (const size of SIZES) {
		const viewer = await phoneOrDesktop(browser, size);
		// Home shows someone's work in the project they last opened in this browser: open it first.
		await viewer.goto(`/studio/projects/${made.project}`);
		await expect(viewer.getByRole('heading', { level: 1 })).toHaveText('Harbour Gazette');
		for (const screen of SCREENS(made)) {
			await viewer.goto(screen.path);
			await viewer.waitForLoadState('networkidle');
			for (const scheme of ['light', 'dark'] as const) {
				await viewer.emulateMedia({ colorScheme: scheme });
				await expect(viewer.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
				// Soft, so one run reports every screen that has a problem.
				expect.soft(await violations(viewer), `${screen.name}, ${size.width}, ${scheme}`).toEqual([]);
				expect.soft((await primaries(viewer)).length, `${screen.name}, ${size.width}: ${(await primaries(viewer)).join(', ')}`).toBeLessThanOrEqual(1);
				if (SHOTS !== undefined && SHOTS !== '') {
					await viewer.screenshot({ path: `${SHOTS}/${screen.name}-${size.width}-${scheme}.png` });
				}
			}
		}
		await viewer.context().close();
	}
});
