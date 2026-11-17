// The Media page (D2) against the real server, worker and object store: the library past one
// page, read on with the cursor; a search over names, titles and alt text, a state and a tag
// narrowing it; files uploaded three at a time through the queue, by the button and by a drop,
// each ending in words (ready, or rejected with its reason); an upload's details — its alt text,
// title, credit and tags saved under If-Match, a conflict said plainly with both ways out, its
// versions, where it is used, a replacement that warns published pages keep the old picture and
// offers the publish request — and the staged delete. Every screen passes axe in light and dark
// and uses plain words; with STUDIO_SHOTS_DIR set, the page is photographed at 360 and 1440 px.

import { mkdirSync } from 'node:fs';
import { join } from 'node:path';
import { deflateSync } from 'node:zlib';
import { expect, type Page, test } from '@playwright/test';
import { api, env, internalTerms, key, newEntry, newProject, publishThroughApi, signIn, violations } from './helpers.ts';

/** A solid-colour PNG of `width`×`height`: real bytes the worker's verifier accepts, wider than
 *  320 pixels when a test needs the thumbnail rendition, and different bytes per colour. */
function png(width: number, height: number, [r, g, b]: [number, number, number]): Buffer {
	const table = Array.from({ length: 256 }, (_, n) => {
		let c = n;
		for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		return c >>> 0;
	});
	const crc = (bytes: Buffer) => {
		let c = 0xffffffff;
		for (const byte of bytes) c = (table[(c ^ byte) & 0xff] ?? 0) ^ (c >>> 8);
		return (c ^ 0xffffffff) >>> 0;
	};
	const chunk = (type: string, data: Buffer) => {
		const length = Buffer.alloc(4);
		length.writeUInt32BE(data.length);
		const body = Buffer.concat([Buffer.from(type, 'ascii'), data]);
		const sum = Buffer.alloc(4);
		sum.writeUInt32BE(crc(body));
		return Buffer.concat([length, body, sum]);
	};
	const header = Buffer.alloc(13);
	header.writeUInt32BE(width, 0);
	header.writeUInt32BE(height, 4);
	header[8] = 8; // bit depth
	header[9] = 2; // truecolour
	const row = Buffer.alloc(1 + width * 3);
	for (let x = 0; x < width; x += 1) row.set([r, g, b], 1 + x * 3);
	const raw = Buffer.concat(Array.from({ length: height }, () => row));
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', header),
		chunk('IDAT', deflateSync(raw)),
		chunk('IEND', Buffer.alloc(0))
	]);
}

const SMALL = png(4, 3, [200, 120, 40]);
const WIDE = png(400, 40, [30, 90, 160]);
const NOT_AN_IMAGE = Buffer.from('this is not an image');

/** Uploads `files` through the API in order (the raw-bytes endpoint, not the page); their ids. */
async function uploadAll(page: Page, project: string, files: { name: string; bytes: Buffer }[]): Promise<string[]> {
	const made = await page.evaluate(
		async ({ path, files }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const ids: string[] = [];
			for (const file of files) {
				const response = await fetch(path, {
					method: 'POST',
					headers: { 'Content-Type': 'image/png', 'X-CSRF-Token': csrf, 'X-Colophon-File-Name': file.name, 'Idempotency-Key': crypto.randomUUID() },
					body: Uint8Array.from(atob(file.bytes), (letter) => letter.charCodeAt(0))
				});
				if (response.status !== 202) throw new Error(`upload ${file.name}: HTTP ${response.status}`);
				ids.push(((await response.json()) as { data: { id: string } }).data.id);
			}
			return ids;
		},
		{ path: `${env(project)}/assets`, files: files.map((file) => ({ name: file.name, bytes: file.bytes.toString('base64') })) }
	);
	return made;
}

/** Waits until none of the project's uploads is still being checked. */
async function settled(page: Page, project: string) {
	await expect
		.poll(async () => ((await api(page, 'GET', `${env(project)}/assets?state=quarantined&limit=100`)).body?.['data'] as unknown[] | undefined)?.length, {
			timeout: 60_000
		})
		.toBe(0);
}

/** One asset's record as the API reads it. */
async function record(page: Page, project: string, asset: string): Promise<Record<string, any>> {
	const read = await api(page, 'GET', `${env(project)}/assets/${asset}`);
	expect(read.status, JSON.stringify(read.body)).toBe(200);
	return read.body?.['data'] as Record<string, any>;
}

/** Changes metadata through the API, as someone else would. */
async function patch(page: Page, project: string, asset: string, body: Record<string, unknown>): Promise<Record<string, any>> {
	const now = await record(page, project, asset);
	const changed = await api(page, 'PATCH', `${env(project)}/assets/${asset}`, body, {
		'If-Match': `"${now['metadata_version']}"`,
		'Idempotency-Key': key()
	});
	expect(changed.status, JSON.stringify(changed.body)).toBe(200);
	return changed.body?.['data'] as Record<string, any>;
}

const SHOTS = process.env['STUDIO_SHOTS_DIR'];

/** The screens photographed for docs/screens: the rest are checked, not kept. */
const PHOTOGRAPHED = ['empty', 'grid', 'details'];

/** axe and plain words in light and dark; with STUDIO_SHOTS_DIR, the main screens photographed at
 *  360 and 1440 px. */
async function screen(page: Page, name: string) {
	await page.waitForLoadState('networkidle');
	for (const scheme of ['light', 'dark'] as const) {
		await page.emulateMedia({ colorScheme: scheme });
		await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
		expect(await violations(page), `${name}, ${scheme}`).toEqual([]);
		if (SHOTS !== undefined && SHOTS !== '' && PHOTOGRAPHED.includes(name)) {
			mkdirSync(SHOTS, { recursive: true });
			for (const width of [360, 1440]) {
				await page.setViewportSize({ width, height: width === 360 ? 780 : 900 });
				await page.waitForTimeout(150);
				await page.screenshot({ path: join(SHOTS, `media-${name}-${width}-${scheme}.png`) });
			}
			await page.setViewportSize({ width: 1280, height: 720 });
		}
	}
	await page.emulateMedia({ colorScheme: 'light' });
	expect(await internalTerms(page), `${name}: internal terms`).toEqual([]);
}

test('an empty library teaches, then pages past one screen, searches, and narrows by state and tag', async ({ page }) => {
	test.setTimeout(180_000);
	await signIn(page);
	const project = await newProject(page, 'Media library');
	await page.goto(`/studio/projects/${project}/media`);
	await expect(page.getByTestId('media-empty')).toContainText('No media yet');
	await expect(page.getByTestId('media-empty-upload')).toBeVisible();
	await screen(page, 'empty');

	// Fifty-two uploads: more than one page of 48. One is wide enough for a thumbnail rendition,
	// one isn't an image at all.
	const files = Array.from({ length: 50 }, (_, index) => ({ name: `photo-${String(index).padStart(2, '0')}.png`, bytes: SMALL }));
	const [first] = await uploadAll(page, project, files);
	const [wide] = await uploadAll(page, project, [{ name: 'harbour-wide.png', bytes: WIDE }]);
	await uploadAll(page, project, [{ name: 'broken.png', bytes: NOT_AN_IMAGE }]);
	await settled(page, project);
	await patch(page, project, wide ?? '', { title: 'Harbour at dawn', alt: 'Fishing boats moored at first light', tags: ['harbour', 'cover'] });

	await page.reload();
	const cards = page.getByTestId('media-card');
	await expect(cards).toHaveCount(48);
	await expect(page.getByTestId('media-count')).toContainText('48 uploads so far');
	// The newest first: the rejected file and the wide one lead.
	await expect(cards.first()).toContainText('broken.png');
	await expect(cards.first().getByTestId('asset-state')).toHaveText('Rejected');
	await expect(cards.first().getByTestId('asset-reason')).not.toBeEmpty();
	// The thumbnail is the 320-pixel rendition, described by the upload's alt text.
	const thumb = cards.nth(1).getByTestId('media-thumb');
	await expect(thumb).toHaveAttribute('src', new RegExp(`/media/v1/${project}/production/${wide}/w320$`));
	await expect(thumb).toHaveAttribute('alt', 'Fishing boats moored at first light');
	await screen(page, 'grid');

	// The next page: a Load more button for keyboards, and scrolling near it reads on by itself.
	await expect(page.getByTestId('media-more')).toBeVisible();
	await page.getByTestId('media-more').scrollIntoViewIfNeeded();
	await expect(cards).toHaveCount(52);
	await expect(page.getByTestId('media-more')).toHaveCount(0);
	await expect(page.getByTestId('media-count')).toHaveText('52 uploads');
	await expect(cards.last()).toContainText(`photo-00.png`);
	expect(first).toBeTruthy();

	// A search reaches the title the API set, past the first page's order.
	await page.getByTestId('media-search').fill('dawn');
	await expect(cards).toHaveCount(1);
	await expect(cards.first()).toContainText('harbour-wide.png');
	await page.getByTestId('media-search').fill('photo-07');
	await expect(cards).toHaveCount(1);
	await page.getByTestId('media-search').fill('nothing called this');
	await expect(page.getByTestId('media-no-match')).toContainText('No uploads match “nothing called this”');
	await screen(page, 'no-match');
	await page.getByTestId('media-clear').click();
	await expect(page.getByTestId('media-search')).toHaveValue('');
	// Back to everything, with both pages it had read.
	await expect(cards).toHaveCount(52);

	// The state filter, then a tag chip.
	await page.getByTestId('media-state').selectOption('rejected');
	await expect(cards).toHaveCount(1);
	await expect(cards.first()).toContainText('broken.png');
	await page.getByTestId('media-state').selectOption('');
	await cards.nth(1).getByRole('button', { name: 'Show uploads tagged harbour' }).click();
	await expect(page.getByTestId('media-tag')).toContainText('#harbour');
	await expect(cards).toHaveCount(1);
	await page.getByRole('button', { name: 'Stop showing only #harbour' }).click();
	await expect(cards).toHaveCount(52);
});

test('files go up three at a time by the button and by a drop, and each says how it ended', async ({ page }) => {
	test.setTimeout(120_000);
	await signIn(page);
	const project = await newProject(page, 'Media uploads');
	await page.goto(`/studio/projects/${project}/media`);
	await expect(page.getByTestId('media-empty')).toBeVisible();

	await page.getByTestId('asset-file').setInputFiles([
		{ name: 'one.png', mimeType: 'image/png', buffer: SMALL },
		{ name: 'two.png', mimeType: 'image/png', buffer: png(5, 5, [10, 200, 10]) },
		{ name: 'three.png', mimeType: 'image/png', buffer: png(6, 5, [10, 10, 200]) },
		{ name: 'four.png', mimeType: 'image/png', buffer: NOT_AN_IMAGE }
	]);
	const items = page.getByTestId('upload-item');
	await expect(items).toHaveCount(4);
	await expect(page.getByTestId('upload-summary')).toHaveText('Uploads finished: 3 ready, 1 rejected', { timeout: 60_000 });
	await expect(items.filter({ hasText: 'four.png' }).getByTestId('upload-status')).toContainText('Rejected: ');
	await expect(items.filter({ hasText: 'one.png' })).toHaveAttribute('data-status', 'ready');
	await expect(page.getByTestId('media-card')).toHaveCount(4);
	await screen(page, 'uploads');

	// A drop anywhere on the page joins the same queue.
	const dropped = await page.evaluateHandle((bytes) => {
		const transfer = new DataTransfer();
		transfer.items.add(new File([Uint8Array.from(atob(bytes), (letter) => letter.charCodeAt(0))], 'dropped.png', { type: 'image/png' }));
		return transfer;
	}, png(7, 7, [90, 90, 90]).toString('base64'));
	await page.getByTestId('media-page').dispatchEvent('dragenter', { dataTransfer: dropped });
	await expect(page.getByTestId('media-drop')).toBeVisible();
	await page.getByTestId('media-page').dispatchEvent('drop', { dataTransfer: dropped });
	await expect(page.getByTestId('media-drop')).toHaveCount(0);
	await expect(items.filter({ hasText: 'dropped.png' })).toHaveAttribute('data-status', 'ready', { timeout: 30_000 });
	await expect(page.getByTestId('media-card')).toHaveCount(5);

	await page.getByTestId('upload-clear').click();
	await expect(page.getByTestId('upload-queue')).toHaveCount(0);
});

test('an upload’s details: words saved, a conflict said plainly, its use, a replacement that warns, and the delete', async ({ page }) => {
	test.setTimeout(180_000);
	await signIn(page);
	const project = await newProject(page, 'Media details');
	const made = await api(
		page,
		'POST',
		`${env(project)}/content-types`,
		{
			slug: 'stories',
			definition: {
				name: 'Stories',
				title_field: 'title',
				fields: [
					{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
					{ id: 'cover', label: 'Cover', type: 'asset', required: false, many: false }
				]
			}
		},
		{ 'Idempotency-Key': key() }
	);
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const [cover, spare] = await uploadAll(page, project, [
		{ name: 'cover.png', bytes: WIDE },
		{ name: 'spare.png', bytes: SMALL }
	]);
	await settled(page, project);
	const story = await newEntry(page, project, 'Harbour story', 'stories', { title: 'Harbour story', cover });
	await publishThroughApi(page, project, 'First story', [story]);

	await page.goto(`/studio/projects/${project}/media`);
	await page.getByTestId('media-card').filter({ hasText: 'cover.png' }).getByTestId('media-open').click();
	const sheet = page.getByTestId('media-details');
	await expect(sheet).toBeVisible();
	await expect(sheet.getByTestId('details-state')).toHaveText('Ready');
	await expect(sheet.getByTestId('details-preview')).toHaveAttribute('src', new RegExp(`/${cover}$`));
	await expect(sheet.getByTestId('details-users')).toContainText('Harbour story');
	await expect(sheet.getByTestId('details-users')).toContainText('Published');
	await expect(sheet.getByTestId('details-versions').locator('li')).toHaveCount(1);

	// The words, saved under If-Match.
	await sheet.getByTestId('details-alt').fill('Boats in the harbour at dawn');
	await sheet.getByTestId('details-title').fill('Harbour');
	await sheet.getByTestId('details-tags').fill('harbour, Cover');
	await expect(sheet.getByTestId('details-status')).toHaveText('Not saved yet');
	await sheet.getByTestId('details-save').click();
	await expect(sheet.getByTestId('details-status')).toHaveText('Saved');
	await expect(sheet.getByTestId('details-tags')).toHaveValue('harbour, cover');
	const saved = await record(page, project, cover ?? '');
	expect(saved['alt']).toBe('Boats in the harbour at dawn');
	expect(saved['tags']).toEqual(['harbour', 'cover']);

	// A bad tag is said before anything is sent.
	await sheet.getByTestId('details-tags').fill('harbour, Not A Tag!');
	await expect(sheet.locator('[aria-invalid="true"]')).toHaveCount(1);
	await expect(sheet.getByTestId('details-save')).toBeDisabled();
	await sheet.getByTestId('details-tags').fill('harbour, cover');

	// Someone else changes it while this person is typing: the save is refused, and the choice is
	// plain. Keep mine keeps their credit and this title.
	await sheet.getByTestId('details-title').fill('Harbour at first light');
	await patch(page, project, cover ?? '', { credit: 'Photo: the harbour master' });
	await sheet.getByTestId('details-save').click();
	await expect(sheet.getByTestId('details-conflict')).toContainText("Someone changed this upload's details while you were editing");
	await screen(page, 'details-conflict');
	await sheet.getByTestId('conflict-mine').click();
	await expect(sheet.getByTestId('details-status')).toHaveText('Saved');
	const kept = await record(page, project, cover ?? '');
	expect([kept['title'], kept['credit']]).toEqual(['Harbour at first light', 'Photo: the harbour master']);
	await expect(sheet.getByTestId('details-credit')).toHaveValue('Photo: the harbour master');
	await sheet.getByTestId('details-title').fill('My title');
	await patch(page, project, cover ?? '', { title: 'Their title' });
	await sheet.getByTestId('details-save').click();
	await sheet.getByTestId('conflict-theirs').click();
	await expect(sheet.getByTestId('details-title')).toHaveValue('Their title');
	await expect(sheet.getByTestId('details-credit')).toHaveValue('Photo: the harbour master');

	// A replacement: the warning first, then the publish request for the published entry.
	await expect(sheet.getByTestId('replace-warning')).toContainText('Published pages keep the old image until you publish again');
	await sheet.getByTestId('replace-file').setInputFiles({ name: 'cover-v2.png', mimeType: 'image/png', buffer: png(420, 40, [160, 40, 30]) });
	await expect(sheet.getByTestId('replace-queued')).toBeVisible();
	await expect(sheet.getByTestId('replace-publish')).toContainText('1 published entry still shows the old image');
	await expect(sheet.getByTestId('details-versions').locator('li')).toHaveCount(2, { timeout: 30_000 });
	// Photographed from the top of the sheet, nothing focused.
	await sheet.getByTestId('details-body').evaluate((body) => body.scrollTo(0, 0));
	await page.evaluate(() => (document.activeElement as HTMLElement | null)?.blur());
	await screen(page, 'details');
	await sheet.getByTestId('replace-ask').click();
	await expect(sheet.getByTestId('replace-request')).toHaveAttribute('href', new RegExp(`/projects/${project}/changesets/`));

	// The staged delete: a used upload can't go; an unused one can.
	await page.keyboard.press('Escape');
	await expect(sheet).toBeHidden();
	await page.getByTestId('media-card').filter({ hasText: 'cover.png' }).getByTestId('delete-asset').click();
	await page.getByTestId('confirm-delete').click();
	await expect(page.getByTestId('media-problem')).toContainText('stories');
	const spareCard = page.getByTestId('media-card').filter({ hasText: 'spare.png' });
	await spareCard.getByTestId('media-open').click();
	await expect(sheet.getByTestId('details-unused')).toBeVisible();
	await sheet.getByTestId('details-delete').click();
	await page.getByTestId('confirm-delete').click();
	await expect(spareCard.getByTestId('asset-state')).toHaveText(/Deleting…|Deleted/, { timeout: 20_000 });
	expect(spare).toBeTruthy();

	// Keyboard only: the card opens with Enter, and Escape gives focus back.
	await page.getByTestId('media-card').filter({ hasText: 'cover.png' }).getByTestId('media-open').focus();
	await page.keyboard.press('Enter');
	await expect(sheet).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(sheet).toBeHidden();
	await expect(page.getByTestId('media-card').filter({ hasText: 'cover.png' }).getByTestId('media-open')).toBeFocused();
});
