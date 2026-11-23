// The publishing flow's screens with real work in them, for the before-and-after record of the
// Studio UI pass (workstream 3): the overview (loaded, and while its reads are slow), a publish
// request in each state, the New publish request dialog with a 120-character title in it, a
// receipt, the releases list and the rollback preview. Each is photographed at a desktop
// (1440×900) and a phone (390×844) width, light and dark, as `<screen>-<width>-<scheme>.png` in
// STUDIO_SHOTS_DIR. Without it the test is skipped: the checks live in publishing.spec.ts.
//
// The world is made through the API only, so the same test photographs any build of the Studio.

import { mkdirSync } from 'node:fs';
import { type Browser, expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn } from './helpers.ts';

const SHOTS = process.env['STUDIO_SHOTS_DIR'] ?? '';

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
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('__Host-colophon_csrf='))?.slice('__Host-colophon_csrf='.length) ?? '';
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

/** A publish request for `entries`, taken as far as `to`; its id and hash. */
async function request(page: Page, project: string, title: string, entries: string[], to: 'open' | 'waiting' | 'approved') {
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title, entries }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const set = made.body?.['data'] as { id: string; hash: string };
	const ifMatch = { 'If-Match': `"${set.hash}"` };
	if (to !== 'open') expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, ifMatch)).status).toBe(200);
	if (to === 'approved') expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/reviews`, { decision: 'approve' }, ifMatch)).status).toBe(200);
	return set;
}

interface World {
	project: string;
	prepared: string;
	waiting: string;
	approved: string;
	scheduled: string;
}

/** A small magazine's project after a few weeks: three releases (the second took an article
 *  down), entries live, live with changes and in draft, and a publish request in every state. */
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
	expect(await publishThroughApi(page, project, 'Storm week and spring planting', [storm, planting, market])).toBe(1);
	expect(await publishThroughApi(page, project, 'Market moves back', [], [market])).toBe(2);
	await retitle(page, project, storm, 'Storm week: how the coast road reopened in four days');
	const lighthouse = await newEntry(page, project, 'Lighthouse open day on Sunday');
	expect(await publishThroughApi(page, project, 'Coast road update and the lighthouse open day', [storm, lighthouse])).toBe(3);
	await retitle(page, project, planting, 'Spring planting guide for small gardens and balconies');
	const interview = await newEntry(page, project, 'Interview: the last letterpress printer in town');
	const library = await newEntry(page, project, LONG_TITLE);
	const ferry = await newEntry(page, project, 'Ferry timetable for the winter season');
	const photos = await newEntry(page, project, 'Photos from the lighthouse open day');
	const prepared = await request(page, project, 'Winter ferry timetable', [ferry], 'open');
	const waiting = await request(page, project, 'The printer interview', [interview], 'waiting');
	const approved = await request(page, project, 'Lighthouse photos', [photos], 'approved');
	const scheduled = await request(page, project, 'Harbour library story', [library], 'approved');
	const tomorrow = new Date(Date.now() + 26 * 3600_000).toISOString().slice(0, 13);
	const at = await api(
		page,
		'POST',
		`${env(project)}/changesets/${scheduled.id}/schedule`,
		{ local_time: `${tomorrow}:00:00`, time_zone: 'UTC' },
		{ 'If-Match': `"${scheduled.hash}"` }
	);
	expect(at.status, JSON.stringify(at.body)).toBe(201);
	return { project, prepared: prepared.id, waiting: waiting.id, approved: approved.id, scheduled: scheduled.id };
}

const SIZES = [
	{ width: 1440, height: 900, phone: false },
	{ width: 390, height: 844, phone: true }
] as const;

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

/** Photographs what is on screen now, light and dark. */
async function both(page: Page, name: string, width: number) {
	for (const scheme of ['light', 'dark'] as const) {
		await page.emulateMedia({ colorScheme: scheme });
		await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
		await page.screenshot({ path: `${SHOTS}/${name}-${width}-${scheme}.png` });
	}
}

/** Opens `path` and waits for its reads to settle. */
async function open(page: Page, path: string) {
	await page.goto(path);
	await page.waitForLoadState('networkidle');
}

test('the publishing screens, photographed', async ({ page, browser }) => {
	test.skip(SHOTS === '', 'set STUDIO_SHOTS_DIR to photograph the publishing screens');
	test.setTimeout(300_000);
	await signIn(page);
	const made = await world(page);
	mkdirSync(SHOTS, { recursive: true });
	const at = (path: string) => `/studio/projects/${made.project}${path}`;

	for (const size of SIZES) {
		const viewer = await viewerAt(browser, size);

		// The overview while its reads are slow: whatever it shows before the answers arrive.
		await viewer.route('**/api/v1/projects/*/environments/**', async (route) => {
			await new Promise((resolve) => setTimeout(resolve, 4_000));
			await route.continue();
		});
		await viewer.goto(at(''));
		await viewer.waitForTimeout(1_000);
		await both(viewer, 'overview-loading', size.width);
		await viewer.unrouteAll({ behavior: 'ignoreErrors' });

		await open(viewer, at(''));
		await both(viewer, 'overview', size.width);

		for (const [name, id] of [
			['request-prepared', made.prepared],
			['request-waiting', made.waiting],
			['request-approved', made.approved],
			['request-scheduled', made.scheduled]
		] as const) {
			await open(viewer, at(`/changesets/${id}`));
			await both(viewer, name, size.width);
		}

		await open(viewer, at('/changesets'));
		await viewer.getByRole('button', { name: 'New publish request' }).first().click();
		await expect(viewer.getByRole('dialog')).toBeVisible();
		await viewer.waitForLoadState('networkidle');
		await expect(viewer.getByRole('dialog')).toContainText('Lighthouse open day');
		await both(viewer, 'new-request', size.width);

		await open(viewer, at('/releases/3'));
		await both(viewer, 'receipt', size.width);

		await open(viewer, at('/releases'));
		await both(viewer, 'releases', size.width);

		// Rolling back to release 1: whatever the button leads to (a request made at once, or a
		// preview that asks first).
		await open(viewer, at('/releases/1'));
		await viewer.getByTestId('rollback').click();
		await viewer.waitForTimeout(1_500);
		await viewer.waitForLoadState('networkidle');
		await both(viewer, 'rollback', size.width);

		await viewer.context().close();
	}
});
