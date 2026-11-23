// What opening and using the entry editor asks the server for (D-184, D-203, after the audit's F9): the
// "Taken down" check is one anonymous public read per entry, not one per keystroke or render; In
// review reads the project's request list once and at most the ten newest waiting requests in
// full, however many there are. Each navigation's requests are logged by path, so a regression
// shows as a number, and the counts are printed for the report.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, publishThroughApi, signIn } from './helpers.ts';

const fold = (path: string) => path.replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, ':id');

/** The live stream's own path; its answer is when it opens (`live.ts`, `streamGate.ts`). */
const STREAM = /^\/api\/v1\/projects\/[^/]+\/environments\/[^/]+\/events$/;

/** Every request the page sends from now on, as `METHOD path` (ids folded to `:id`), in order, with
 *  `OPEN stream` where the live stream's answer arrived. */
function logged(page: Page): string[] {
	const seen: string[] = [];
	page.on('request', (request) => {
		const url = new URL(request.url());
		if (!url.pathname.startsWith('/api/') && !url.pathname.startsWith('/content/') && url.pathname !== '/me') return;
		seen.push(`${request.method()} ${fold(url.pathname)}`);
	});
	page.on('response', (response) => {
		if (STREAM.test(new URL(response.url()).pathname)) seen.push('OPEN stream');
	});
	return seen;
}

/** Whether the first `read` went before the stream opened: the gate waits only OPEN_WAIT_MS for it
 *  (D-203), and a read that went ahead is read once more when the stream opens, by design. */
function wentEarly(seen: string[], read: string): boolean {
	const first = seen.indexOf(read);
	const open = seen.indexOf('OPEN stream');
	return first !== -1 && (open === -1 || first < open);
}

function counts(seen: string[]): Record<string, number> {
	const out: Record<string, number> = {};
	for (const one of seen) out[one] = (out[one] ?? 0) + 1;
	return out;
}

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
	await page.waitForTimeout(3_000); // a first-open catch-up, if one ran, would be in by now
	const opened = counts(seen);
	const publicReads = (list: Record<string, number>) => list['GET /content/v1/:id/production/articles/:id'] ?? 0;
	// Opening reads it once: reads wait for the stream (D-203), so nothing is read again when it opens.
	expect(publicReads(opened), JSON.stringify(opened, null, 1)).toBe(1);

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
	await page.waitForTimeout(3_000);
	const opened = counts(seen);
	const full = opened['GET /api/v1/projects/:id/environments/production/changesets/:id'] ?? 0;
	const lists = opened['GET /api/v1/projects/:id/environments/production/changesets'] ?? 0;
	expect(full, JSON.stringify(opened, null, 1)).toBeLessThanOrEqual(10);
	// Once — or, when the stream was slower than the gate's wait and the read went ahead, once more
	// at the open (streamGate.ts). Never a read per request.
	const listRead = 'GET /api/v1/projects/:id/environments/production/changesets';
	expect(lists, JSON.stringify(seen, null, 1)).toBeLessThanOrEqual(wentEarly(seen, listRead) ? 2 : 1);
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
	await page.waitForTimeout(3_000); // a first-open catch-up, if one ran, would be in by now
	console.log(`requests opening a live entry: ${JSON.stringify(counts(seen))}`);
	// A live entry needs no public read at all: only a draft might be "taken down".
	expect(counts(seen)['GET /content/v1/:id/production/articles/:id'] ?? 0).toBe(0);
	// Every resource exactly once, the stream's open included (D-203: reads wait for it, so it re-reads
	// nothing). No allowance: a new duplicate fails here.
	const twice = Object.entries(counts(seen)).filter(([, times]) => times > 1);
	expect(twice, JSON.stringify(counts(seen), null, 1)).toEqual([]);
});
