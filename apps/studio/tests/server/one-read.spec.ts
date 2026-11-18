// One read per resource per navigation (F9, P3-12, workstream 4 slice 1), against the real server: each screen
// asks the API once for each thing it needs, on a full page load and on a move inside the Studio, because the
// project's reads wait for the live stream (`streamGate.ts`) and the sidebar, layout and page share their reads
// (`queries.ts`). The guarantee the old catch-up gave still holds: a commit made while the stream is slow to open
// shows up without a reload. And a long release title stays inside its home card (F11).

import { expect, type Page, test } from '@playwright/test';
import { newEntry, newProject, publishThroughApi, signIn } from './helpers.ts';

/** Every API read the page sends from now on, ids folded to `:id`, the stream itself left out. */
function reads(page: Page): string[] {
	const seen: string[] = [];
	page.on('request', (request) => {
		const url = new URL(request.url());
		if (request.method() !== 'GET' || !url.pathname.startsWith('/api/') || url.pathname.endsWith('/events')) return;
		seen.push(`${url.pathname.replace(/[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}/g, ':id')}${url.search}`);
	});
	return seen;
}

const twice = (seen: string[]) => [...new Set(seen.filter((one) => seen.filter((other) => other === one).length > 1))];

test('each screen reads each thing once, on a full load and on a move inside the Studio', async ({ page }) => {
	test.setTimeout(120_000);
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Counted once');
	await publishThroughApi(page, project, 'One', [entry]);
	// The setup's own writes announce themselves on the stream (D-108): let them drain first.
	await page.waitForTimeout(3_000);
	const screens = [
		['overview', `/studio/projects/${project}`],
		['articles', `/studio/projects/${project}/articles`],
		['entry', `/studio/projects/${project}/articles/${entry}`],
		['requests', `/studio/projects/${project}/changesets`],
		['releases', `/studio/projects/${project}/releases`],
		['receipt', `/studio/projects/${project}/releases/1`]
	] as const;
	const report: Record<string, number> = {};
	for (const [name, path] of screens) {
		const seen = reads(page);
		await page.goto(path);
		await page.waitForLoadState('networkidle');
		await page.waitForTimeout(500);
		report[`${name} (load)`] = seen.length;
		expect(twice(seen), `${name}, full load: read twice`).toEqual([]);
	}
	// Inside the Studio the stream is already open: moving between screens reads each thing at most once.
	await page.goto(`/studio/projects/${project}`);
	await page.waitForLoadState('networkidle');
	for (const [name, link] of [
		['articles', /^Articles$/],
		['requests', /^Publish requests$/],
		['releases', /^Releases$/]
	] as const) {
		const seen = reads(page);
		await page.getByRole('link', { name: link }).first().click();
		await page.waitForLoadState('networkidle');
		await page.waitForTimeout(500);
		report[`${name} (move)`] = seen.length;
		expect(twice(seen), `${name}, moving to it: read twice`).toEqual([]);
	}
	console.log(`READS ${JSON.stringify(report)}`);
});

test('a commit made while the stream is slow to open still shows, without a reload', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Late stream');
	// Hold the stream back past the reads' wait, so the page's reads go first (the early path).
	await page.route('**/events', async (route) => {
		await new Promise((resolve) => setTimeout(resolve, 1_500));
		await route.continue();
	});
	await page.goto(`/studio/projects/${project}/releases`);
	await page.waitForLoadState('domcontentloaded');
	// A release committed after the page's reads, before the stream listens: its hint goes to nobody.
	expect(await publishThroughApi(page, project, 'Made while the stream opened', [entry])).toBe(1);
	// The first open re-reads what went early, so the release appears without a reload.
	await expect(page.getByText('Made while the stream opened').first()).toBeVisible({ timeout: 10_000 });
});

test('a long release title stays on one line inside its home card (F11)', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const title = 'Storm week: how the coast road at Penmarch reopened in four days, and what the village did while the tide came '.padEnd(120, 'x').slice(0, 120);
	const entry = await newEntry(page, project, 'For the long title');
	await publishThroughApi(page, project, title, [entry]);
	for (const viewport of [
		{ width: 1440, height: 900 },
		{ width: 390, height: 844 }
	]) {
		await page.setViewportSize(viewport);
		await page.goto('/studio/');
		const list = page.getByTestId('my-releases-list');
		await expect(list).toBeVisible();
		const fits = await list.evaluate((ul) => {
			const card = ul.closest('section')!;
			const row = ul.querySelector('a')!;
			const line = row.querySelector('.truncate') as HTMLElement;
			return {
				rowInCard: row.getBoundingClientRect().right <= card.getBoundingClientRect().right + 0.5,
				oneLine: line.getBoundingClientRect().height < 24,
				truncated: line.scrollWidth > line.clientWidth,
				pageScroll: document.documentElement.scrollWidth > innerWidth
			};
		});
		expect(fits, `${viewport.width}px`).toEqual({ rowInCard: true, oneLine: true, truncated: true, pageScroll: false });
	}
});
