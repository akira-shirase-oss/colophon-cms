// What opening and using the entry editor asks the server for (D-184, after the audit's F9): the
// "Taken down" check is one anonymous public read per entry, not one per keystroke or render; In
// review reads the project's request list once and at most the ten newest waiting requests in
// full, however many there are. Each navigation's requests are logged by path, so a regression
// shows as a number, and the counts are printed for the report.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn } from './helpers.ts';

/** Every request the page sends from now on, as `METHOD path` (ids folded to `:id`). */
function logged(page: Page): string[] {
	const seen: string[] = [];
	page.on('request', (request) => {
		const url = new URL(request.url());
		if (!url.pathname.startsWith('/api/') && !url.pathname.startsWith('/content/') && url.pathname !== '/me') return;
		const path = url.pathname.replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, ':id');
		seen.push(`${request.method()} ${path}`);
	});
	return seen;
}

/** The page's own reads: everything sent before the live stream opened. After it, the known catch-up
 *  (`KNOWN_DUPLICATES.catchUp`) may read each of them once more. */
function beforeStream(seen: string[]): string[] {
	const at = seen.findIndex((one) => one.endsWith('/events'));
	return at < 0 ? seen : seen.slice(0, at);
}

function counts(seen: string[]): Record<string, number> {
	const out: Record<string, number> = {};
	for (const one of seen) out[one] = (out[one] ?? 0) + 1;
	return out;
}

/** Reads beyond "once per navigation" that are known and named, so the test fails if a new one appears AND if a
 *  known one goes away (then its allowance must be removed here). One today: F9, workstream 4 — the live
 *  stream's first open re-reads what the page has just loaded (`lib/live.ts`'s catch-up), so every read the page
 *  made can come once more after `/events` opens. Workstream 4's first slice removes it, and this entry with it. */
const KNOWN_DUPLICATES = {
	catchUp: 'F9, workstream 4: the first SSE open refetches what the page just loaded'
} as const;

test('a taken-down entry reads the public API once, not per keystroke or render', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Ferry strike notice');
	const generation = await publishThroughApi(page, project, 'Strike', [entry]);
	await publishThroughApi(page, project, 'Down', [], [entry]);
	expect(generation).toBe(1);

	const seen = logged(page);
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'taken_down');
	await page.waitForLoadState('networkidle');
	await page.waitForTimeout(3_000); // the catch-up, if any, has run
	const opened = counts(seen);
	const publicReads = (list: Record<string, number>) => list['GET /content/v1/:id/production/articles/:id'] ?? 0;
	// Opening reads it once (the known first-open catch-up may read it once more: KNOWN_DUPLICATES).
	expect(publicReads(counts(beforeStream(seen))), JSON.stringify(opened, null, 1)).toBe(1);

	// Forty keystrokes in the title and two saves: still the one public read.
	const title = page.getByRole('heading', { level: 1 }).getByRole('textbox', { name: 'Title' });
	await title.click();
	await page.keyboard.press('End');
	await page.keyboard.type(' — the Friday sailings are cancelled', { delay: 15 });
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.waitForLoadState('networkidle');
	const typed = counts(seen);
	// Typing and saving add no public read at all.
	expect(publicReads(typed), JSON.stringify(typed, null, 1)).toBe(publicReads(opened));
	console.log(`requests opening a taken-down entry: ${JSON.stringify(opened)}`);
	console.log(`after 40 keystrokes and saves: ${JSON.stringify(typed)}`);
});

test('In review reads the request list once and at most ten waiting requests in full', async ({ page }) => {
	test.setTimeout(120_000);
	await signIn(page);
	const project = await newProject(page);
	// Fourteen requests waiting for review, the oldest of them holding the entry opened below.
	const target = await newEntry(page, project, 'The printer interview');
	const ids: string[] = [];
	for (let i = 0; i < 14; i += 1) {
		const entry = i === 0 ? target : await newEntry(page, project, `Waiting ${i}`);
		const made = await api(page, 'POST', `${env(project)}/changesets`, { title: `Request ${i}`, entries: [entry] }, { 'Idempotency-Key': key() });
		expect(made.status, JSON.stringify(made.body)).toBe(201);
		const set = made.body?.['data'] as { id: string; hash: string };
		expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, { 'If-Match': `"${set.hash}"` })).status).toBe(200);
		ids.push(set.id);
	}
	const seen = logged(page);
	const newest = ids.at(-1) ?? '';
	await page.goto(`/studio/projects/${project}/articles/${await newEntry(page, project, 'Not waiting')}`);
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	await page.waitForLoadState('networkidle');
	// The page's own reads (the known first-open catch-up may repeat each once: KNOWN_DUPLICATES).
	const opened = counts(beforeStream(seen));
	const full = opened['GET /api/v1/projects/:id/environments/production/changesets/:id'] ?? 0;
	const lists = opened['GET /api/v1/projects/:id/environments/production/changesets'] ?? 0;
	expect(full, JSON.stringify(opened, null, 1)).toBeLessThanOrEqual(10);
	expect(lists, JSON.stringify(opened, null, 1)).toBeLessThanOrEqual(1);
	console.log(`requests opening an entry with 14 requests waiting: ${JSON.stringify(opened)}`);
	expect(newest).not.toBe('');

	// The newest request's entry reads In review; the bound is the newest ten, so the oldest
	// one's entry is said plainly not to be found in them (it reads as a draft), never a fan-out.
	const newestEntry = await page.evaluate(async (path) => ((await (await fetch(path)).json()) as { data: { items: { entry_id: string }[] } }).data.items[0]?.entry_id, `${env(project)}/changesets/${newest}`);
	await page.goto(`/studio/projects/${project}/articles/${newestEntry}`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'review');
});


test('opening a live entry: each resource read once per navigation', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Regatta programme');
	await publishThroughApi(page, project, 'Regatta', [entry]);
	await page.goto(`/studio/projects/${project}`);
	await page.waitForLoadState('networkidle');
	// The setup's own writes announce themselves on the live stream (D-108): let them drain first.
	await page.waitForTimeout(3_000);
	const seen = logged(page);
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(page.getByTestId('entry-state')).toHaveAttribute('data-state', 'live');
	await page.waitForLoadState('networkidle');
	// The page's own reads come before the stream opens; whatever follows the open is the catch-up.
	const open = seen.findIndex((one) => one.endsWith('/events'));
	const load = open < 0 ? seen : seen.slice(0, open);
	const catchUp = open < 0 ? [] : seen.slice(open + 1).filter((one) => !one.endsWith('/events'));
	console.log(`requests opening a live entry: ${JSON.stringify({ load: counts(load), catchUp: counts(catchUp) })}`);
	// A live entry needs no public read at all: only a draft might be "taken down".
	expect(counts(seen)['GET /content/v1/:id/production/articles/:id'] ?? 0).toBe(0);
	// The page itself: every resource exactly once. No allowance here, so a new duplicate fails.
	const twiceOnLoad = Object.entries(counts(load)).filter(([, times]) => times > 1);
	expect(twiceOnLoad, JSON.stringify(counts(load), null, 1)).toEqual([]);
	// The known catch-up (KNOWN_DUPLICATES.catchUp): it only re-reads what the page loaded, each at most once…
	const unexpected = catchUp.filter((one) => !load.includes(one));
	expect(unexpected, `the catch-up read something the page never loaded (${KNOWN_DUPLICATES.catchUp})`).toEqual([]);
	const twiceInCatchUp = Object.entries(counts(catchUp)).filter(([, times]) => times > 1);
	expect(twiceInCatchUp, 'the catch-up read the same thing twice').toEqual([]);
	// …and it is still there: when F9 is fixed this fails, and the allowance above must go with it.
	expect(catchUp.length, `the known duplicate is gone (${KNOWN_DUPLICATES.catchUp}): remove KNOWN_DUPLICATES.catchUp and assert no catch-up reads`).toBeGreaterThan(0);
});
