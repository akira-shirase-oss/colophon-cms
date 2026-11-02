// Scheduled publishing through the testbed (D-060): an approved changeset scheduled a few seconds
// ahead is published by the worker as release 1; a New York time the clocks show twice is refused
// with both candidates, then scheduled with a choice and cancelled; a past time is refused.

import { expect, type Page, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

/** A project with an approved changeset of one article: (project, changeset, hash). */
async function setup(page: Page, title: string): Promise<{ project: string; changeset: string; hash: string }> {
	return page.evaluate(
		async ({ slug, title }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf };
			const post = async (path: string, body: unknown, headers: Record<string, string> = {}) => {
				const reply = await fetch(`/server${path}`, { method: 'POST', headers: { ...json, ...headers }, body: JSON.stringify(body) });
				return (await reply.json()) as { data: { id: string; hash: string } };
			};
			const project = (await post('/api/v1/projects', { slug, name: 'Schedules testbed' })).data.id;
			const base = `/api/v1/projects/${project}/environments/production`;
			const doc = { type: 'doc', content: [{ type: 'paragraph' }] };
			const entry = await post(`${base}/collections/articles/entries`, { title, body: { schema: 'colophon/rich-text', version: 1, doc } }, { 'Idempotency-Key': crypto.randomUUID() });
			const set = await post(`${base}/changesets`, { title, entries: [entry.data.id] }, { 'Idempotency-Key': crypto.randomUUID() });
			const ifMatch = { 'If-Match': `"${set.data.hash}"` };
			await post(`${base}/changesets/${set.data.id}/submit`, undefined, ifMatch);
			await post(`${base}/changesets/${set.data.id}/reviews`, { decision: 'approve' }, ifMatch);
			return { project, changeset: set.data.id, hash: set.data.hash };
		},
		{ slug: `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`, title }
	);
}

/** The seconds `zone` sits east of UTC at `instant` (ms), via Intl. */
function offsetSeconds(zone: string, instant: number): number {
	const parts = new Intl.DateTimeFormat('en-US', {
		timeZone: zone,
		hourCycle: 'h23',
		year: 'numeric',
		month: '2-digit',
		day: '2-digit',
		hour: '2-digit',
		minute: '2-digit',
		second: '2-digit',
	}).formatToParts(new Date(instant));
	const field = (type: Intl.DateTimeFormatPartTypes) => Number(parts.find((part) => part.type === type)?.value);
	const asUtc = Date.UTC(field('year'), field('month') - 1, field('day'), field('hour'), field('minute'), field('second'));
	return Math.round((asUtc - instant) / 1000);
}

/**
 * A local time the clocks show twice within `days` days of `now` (ms), across zones that fall back
 * sometime in the year: (zone, `YYYY-MM-DDTHH:MM`, earlier instant, later instant as ISO). `null`
 * when no backward clock change fits the window — some stretches of the year have none.
 */
function nextAmbiguous(now: number, days: number): { zone: string; local: string; earlier: string; later: string } | null {
	const zones = [
		'America/New_York',
		'America/Santiago',
		'Australia/Lord_Howe',
		'Pacific/Auckland',
		'Pacific/Fiji',
		'Africa/Casablanca',
		'Europe/London',
		'America/Nuuk',
		'America/Havana',
		'Asia/Gaza',
		'Australia/Sydney',
		'America/Asuncion',
	];
	const horizon = now + days * 24 * 60 * 60 * 1000;
	for (const zone of zones) {
		// Probe hour by hour for an offset change, then close in to the minute.
		for (let at = now; at < horizon; at += 60 * 60 * 1000) {
			if (offsetSeconds(zone, at) === offsetSeconds(zone, Math.min(at + 60 * 60 * 1000, horizon))) continue;
			let lo = at;
			let hi = at + 60 * 60 * 1000;
			while (hi - lo > 60_000) {
				const mid = Math.floor((lo + hi) / 2 / 60_000) * 60_000;
				if (offsetSeconds(zone, mid) === offsetSeconds(zone, lo)) lo = mid;
				else hi = mid;
			}
			if (hi > horizon) break;
			const before = offsetSeconds(zone, hi - 1000);
			const after = offsetSeconds(zone, hi);
			if (after >= before) continue;
			// The clocks go back at hi: local times [hi + after, hi + before) happen twice. A whole
			// minute inside is the local time asked for; the two instants it can mean are it minus
			// each offset.
			const mid = hi + (after + (before - after) / 2) * 1000;
			const localAt = mid - (mid % 60_000);
			const local = new Date(localAt).toISOString().slice(0, 16);
			const instant = (at: number) => new Date(at).toISOString().slice(0, 19) + 'Z';
			return { zone, local, earlier: instant(localAt - before * 1000), later: instant(localAt - after * 1000) };
		}
	}
	return null;
}

test('schedule, publish on time, choose a doubled time, cancel, refuse the past', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const { project, changeset, hash } = await setup(page, 'Scheduled note');
	await page.goto('/schedules');
	await page.getByLabel('Project').selectOption(project);
	await page.getByLabel('Changeset id').fill(changeset);
	await page.getByLabel('If-Match hash').fill(hash);

	// Four seconds ahead in UTC: the worker publishes it.
	await page.getByLabel('Local time').fill(new Date(Date.now() + 4000).toISOString().slice(0, 19));
	await page.getByRole('button', { name: 'Schedule', exact: true }).click();
	await expect(page.getByTestId('message')).toContainText('scheduled for');
	await expect(async () => {
		await page.getByRole('button', { name: 'Refresh' }).click();
		await expect(page.getByTestId('schedules').locator('li').first()).toHaveAttribute('data-state', 'published', { timeout: 1000 });
	}).toPass({ timeout: 20_000 });
	await expect(page.getByTestId('schedules').locator('li').first()).toContainText('as release 1');

	// A doubled local time is refused with both candidates, then scheduled with a choice and
	// cancelled. The window is the approval's week and no clock change may fall inside it; when
	// none does that part is skipped, and a plain pending schedule takes the cancel.
	const found = nextAmbiguous(Date.now(), 6);
	const second = await setup(page, found ? 'Doubled note' : 'Pending note');
	await page.reload();
	await page.getByLabel('Project').selectOption(second.project);
	await page.getByLabel('Changeset id').fill(second.changeset);
	await page.getByLabel('If-Match hash').fill(second.hash);
	if (found) {
		await page.getByLabel('Local time').fill(found.local);
		await page.getByLabel('Time zone').fill(found.zone);
		await page.getByRole('button', { name: 'Schedule', exact: true }).click();
		await expect(page.getByTestId('message')).toHaveText('422 validation_failed (ambiguous_local_time)');
		await expect(page.getByTestId('candidates')).toHaveText(`${found.earlier} or ${found.later}`);
		await page.getByLabel('Choice').selectOption('earlier');
		await page.getByRole('button', { name: 'Schedule', exact: true }).click();
		await expect(page.getByTestId('message')).toHaveText(`scheduled for ${found.earlier}`);
	} else {
		await page.getByLabel('Time zone').fill('UTC');
		await page.getByLabel('Local time').fill(new Date(Date.now() + 5 * 24 * 60 * 60 * 1000).toISOString().slice(0, 16));
		await page.getByRole('button', { name: 'Schedule', exact: true }).click();
		await expect(page.getByTestId('message')).toContainText('scheduled for');
	}
	await page.getByRole('button', { name: `Cancel ${found ? 'Doubled note' : 'Pending note'}` }).click();
	await expect(page.getByTestId('message')).toHaveText('cancelled');

	await page.getByLabel('Choice').selectOption('');
	await page.getByLabel('Time zone').fill('UTC');
	await page.getByLabel('Local time').fill('2020-01-01T00:00');
	await page.getByRole('button', { name: 'Schedule', exact: true }).click();
	await expect(page.getByTestId('message')).toHaveText('422 validation_failed (in_the_past)');
});
