// Images inside rich text (gap 3 D1), in the Studio against the real server, object store and
// worker: an image is chosen from the media library (searched, then described — alt text is
// required, and a file name is questioned), inserted as the real upload's id, drawn in the editor
// as its picture, then its alt text edited (by button and by Enter), replaced and removed. An
// image naming no upload is refused by the server and said in place, in plain words. With
// STUDIO_SHOTS_DIR set it photographs the dialog and the editor at 1440 and 360 px, light and dark.

import { deflateSync } from 'node:zlib';
import { expect, type Locator, type Page, test } from '@playwright/test';
import { api, env, newEntry, newProject, shot, signIn, violations } from './helpers.ts';

/** A real `width`×`height` PNG (an RGB gradient) the worker's verifier accepts. */
function png(width: number, height: number, hue: [number, number, number]): Buffer {
	const crcTable = Array.from({ length: 256 }, (_, n) => {
		let c = n;
		for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		return c >>> 0;
	});
	const crc = (bytes: Buffer) => {
		let c = 0xffffffff;
		for (const byte of bytes) c = (crcTable[(c ^ byte) & 0xff] ?? 0) ^ (c >>> 8);
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
	header.set([8, 2, 0, 0, 0], 8);
	const rows = Buffer.alloc((width * 3 + 1) * height);
	for (let y = 0; y < height; y += 1) {
		const row = y * (width * 3 + 1);
		for (let x = 0; x < width; x += 1) {
			const at = row + 1 + x * 3;
			rows[at] = Math.round((hue[0] * x) / width);
			rows[at + 1] = Math.round((hue[1] * y) / height);
			rows[at + 2] = hue[2];
		}
	}
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', header),
		chunk('IDAT', deflateSync(rows)),
		chunk('IEND', Buffer.alloc(0))
	]);
}

/** An upload through the API; its id once the worker has made it usable. */
async function uploaded(page: Page, project: string, name: string, bytes: Buffer): Promise<string> {
	const made = await page.evaluate(
		async ({ project, name, data }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(`/api/v1/projects/${project}/environments/production/assets`, {
				method: 'POST',
				headers: { 'Content-Type': 'image/png', 'X-CSRF-Token': csrf, 'X-Colophon-File-Name': name, 'Idempotency-Key': crypto.randomUUID() },
				body: Uint8Array.from(atob(data), (letter) => letter.charCodeAt(0))
			});
			return { status: response.status, id: ((await response.json().catch(() => null))?.data?.id as string) ?? '' };
		},
		{ project, name, data: bytes.toString('base64') }
	);
	expect(made.status).toBe(202);
	for (let attempt = 0; attempt < 100; attempt += 1) {
		const read = await api(page, 'GET', `${env(project)}/assets/${made.id}`);
		if (read.body?.['data']?.state === 'usable') return made.id;
		await page.waitForTimeout(200);
	}
	throw new Error('the upload never settled');
}

/** The images the saved draft's body names, as stored. */
async function storedImages(page: Page, project: string, entry: string): Promise<{ assetId: string; alt: string }[]> {
	const read = await api(page, 'GET', `${env(project)}/collections/articles/entries/${entry}`);
	const content = (read.body?.['data']?.content?.body?.doc?.content ?? []) as { type: string; attrs?: { assetId: string; alt: string } }[];
	return content.filter((node) => node.type === 'image').map((node) => node.attrs ?? { assetId: '', alt: '' });
}

const status = (page: Page) => page.getByTestId('save-status');

/** The article's body: its editor, toolbar and image views. */
const body = (page: Page): Locator => page.locator('[data-testid="rich-field"][data-field="body"]');

test('an image is chosen from the library, described, inserted as the real upload, then edited, replaced and removed', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const harbour = await uploaded(page, project, 'harbour.png', png(960, 540, [40, 120, 200]));
	const notes = await uploaded(page, project, 'notes.png', png(640, 480, [220, 160, 60]));
	const entry = await newEntry(page, project, 'Harbour notes');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await body(page).locator('.tiptap').click();
	await page.keyboard.type('Morning at the harbour.');

	// The library: searched, then the upload chosen.
	const opener = body(page).getByRole('button', { name: 'Image', exact: true });
	await opener.click();
	const pick = page.getByRole('dialog', { name: 'Insert an image' });
	await expect(pick).toBeVisible();
	await expect(pick.getByTestId('asset-search')).toBeFocused();
	await expect(pick.getByTestId('asset-option')).toHaveCount(2);
	await pick.getByTestId('asset-search').fill('harb');
	await expect(pick.getByTestId('asset-option')).toHaveCount(1);
	await pick.getByTestId('asset-search').fill('nothing like it');
	await expect(pick.getByTestId('asset-no-match')).toContainText('No uploads match');
	await pick.getByRole('button', { name: 'Clear the search' }).click();
	await expect(pick.getByTestId('asset-option')).toHaveCount(2);
	expect(await violations(page), 'library step').toEqual([]);
	await shot(page, 'editor-image-library-1440-light');
	await pick.getByTestId('asset-option').filter({ hasText: 'harbour.png' }).click();

	// Described: required, and a file name is questioned.
	const describe = page.getByRole('dialog', { name: 'Describe the image' });
	await expect(describe).toBeVisible();
	const insert = describe.getByRole('button', { name: 'Insert image' });
	await expect(insert).toBeDisabled();
	await describe.getByLabel('Alt text').fill('harbour.png');
	await expect(describe.getByTestId('alt-file-name')).toBeVisible();
	await describe.getByLabel('Alt text').fill('Fishing boats moored in a small harbour at dawn');
	await expect(describe.getByTestId('alt-file-name')).toHaveCount(0);
	expect(await violations(page), 'describe step').toEqual([]);
	await shot(page, 'editor-image-describe-1440-light');
	await insert.click();
	await expect(page.getByRole('dialog')).toHaveCount(0);

	// Drawn as the real picture, and stored as the upload's id and the alt text, nothing else.
	const image = body(page).getByTestId('rich-image');
	await expect(image).toHaveAttribute('data-state', 'usable');
	await expect(image.locator('img')).toHaveAttribute('src', new RegExp(`/media/v1/${project}/production/${harbour}`));
	await expect(image.getByTestId('rich-image-alt')).toHaveText('Fishing boats moored in a small harbour at dawn');
	await expect.poll(() => storedImages(page, project, entry), { timeout: 10_000 }).toEqual([{ assetId: harbour, alt: 'Fishing boats moored in a small harbour at dawn' }]);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	expect(await violations(page), 'editor with an image').toEqual([]);
	await shot(page, 'editor-image-1440-light');

	// Edit alt text, by its button.
	await image.getByTestId('rich-image-edit').click();
	const edit = page.getByRole('dialog', { name: 'Edit alt text' });
	await expect(edit.getByLabel('Alt text')).toHaveValue('Fishing boats moored in a small harbour at dawn');
	await edit.getByLabel('Alt text').fill('Fishing boats moored in a small harbour at sunrise');
	await edit.getByRole('button', { name: 'Save alt text' }).click();
	await expect.poll(() => storedImages(page, project, entry), { timeout: 10_000 }).toEqual([{ assetId: harbour, alt: 'Fishing boats moored in a small harbour at sunrise' }]);

	// By keyboard: the image selected, Enter edits; Escape closes and changes nothing.
	await image.locator('img').click();
	await page.keyboard.press('Enter');
	await expect(page.getByRole('dialog', { name: 'Edit alt text' })).toBeVisible();
	await page.keyboard.press('Escape');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	expect(await storedImages(page, project, entry)).toEqual([{ assetId: harbour, alt: 'Fishing boats moored in a small harbour at sunrise' }]);

	// Replace: another upload, the words kept to check.
	await image.getByTestId('rich-image-replace').click();
	const replace = page.getByRole('dialog', { name: 'Replace the image' });
	await replace.getByTestId('asset-option').filter({ hasText: 'notes.png' }).click();
	const words = page.getByRole('dialog', { name: 'Describe the image' });
	await expect(words.getByLabel('Alt text')).toHaveValue('Fishing boats moored in a small harbour at sunrise');
	await words.getByLabel('Alt text').fill('A notebook of tide times on a café table');
	await words.getByRole('button', { name: 'Replace image' }).click();
	await expect(image.locator('img')).toHaveAttribute('src', new RegExp(`/media/v1/${project}/production/${notes}`));
	await expect.poll(() => storedImages(page, project, entry), { timeout: 10_000 }).toEqual([{ assetId: notes, alt: 'A notebook of tide times on a café table' }]);

	// Phone width and dark: the same editor, still usable.
	for (const [width, height, scheme] of [
		[1440, 900, 'dark'],
		[360, 780, 'light'],
		[360, 780, 'dark']
	] as const) {
		await page.setViewportSize({ width, height });
		await page.emulateMedia({ colorScheme: scheme });
		await expect(image.getByTestId('rich-image-edit')).toBeVisible();
		expect(await violations(page), `editor ${width} ${scheme}`).toEqual([]);
		await shot(page, `editor-image-${width}-${scheme}`);
	}
	await opener.click();
	await expect(page.getByRole('dialog', { name: 'Insert an image' }).getByTestId('asset-option')).toHaveCount(2);
	expect(await violations(page), 'library at 360, dark').toEqual([]);
	await shot(page, 'editor-image-library-360-dark');
	// Escape closes the dialog and gives focus back to the button that opened it.
	await page.keyboard.press('Escape');
	await expect(page.getByRole('dialog')).toHaveCount(0);
	await expect(opener).toBeFocused();
	await page.setViewportSize({ width: 1440, height: 900 });
	await page.emulateMedia({ colorScheme: 'light' });

	// Remove: gone from the editor and from what is stored.
	await image.getByTestId('rich-image-remove').click();
	await expect(body(page).getByTestId('rich-image')).toHaveCount(0);
	await expect.poll(() => storedImages(page, project, entry), { timeout: 10_000 }).toEqual([]);
});

test('an image that names no upload is refused by the server and said in place, in plain words', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Lost picture');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	// Pasted Colophon content keeps its images; this one names an id no upload has (an old
	// placeholder, say).
	const stranger = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d';
	const editor = body(page).locator('.tiptap');
	await editor.click();
	await editor.evaluate((element, html) => {
		const data = new DataTransfer();
		data.setData('text/html', html);
		element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: data, bubbles: true, cancelable: true }));
	}, `<p>Before</p><figure data-colophon-image data-asset-id="${stranger}" data-alt="An old placeholder"></figure>`);

	const image = body(page).getByTestId('rich-image');
	await expect(image).toHaveAttribute('data-state', 'missing');
	await expect(image.getByTestId('rich-image-state')).toHaveText('Missing');
	await expect(status(page)).toHaveAttribute('data-status', 'refused', { timeout: 10_000 });
	const refusal = body(page).getByTestId('rich-refusal');
	await expect(refusal).toContainText("This image isn't in the media library. Choose a replacement or remove it.");
	await refusal.getByText('Technical details').click();
	await expect(refusal).toContainText('asset_not_found');
	expect(await violations(page), 'refused image').toEqual([]);
	await shot(page, 'editor-image-missing-1440-light');

	// Removing it lets the draft save again.
	await image.getByTestId('rich-image-remove').click();
	await expect(status(page)).toHaveAttribute('data-status', 'saved', { timeout: 10_000 });
	await expect(body(page).getByTestId('rich-refusal')).toHaveCount(0);
});

test('an image file pasted into the editor is uploaded, described, and shows Checking… until it is ready', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Pasted picture');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	const editor = body(page).locator('.tiptap');
	await editor.click();
	await editor.evaluate((element, data) => {
		const bytes = Uint8Array.from(atob(data), (letter) => letter.charCodeAt(0));
		const transfer = new DataTransfer();
		transfer.items.add(new File([bytes], 'pasted.png', { type: 'image/png' }));
		element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: transfer, bubbles: true, cancelable: true }));
	}, png(800, 450, [90, 200, 120]).toString('base64'));

	// Uploaded at once, then described before it goes in.
	const describe = page.getByRole('dialog', { name: 'Describe the image' });
	// The upload has landed (not only begun: "Uploading pasted.png…" names the file too).
	await expect(describe.getByTestId('image-chosen')).toContainText('pasted.png');
	await expect(describe.getByTestId('image-chosen')).not.toContainText('Uploading');
	await describe.getByLabel('Alt text').fill('A green field under an evening sky');
	await describe.getByRole('button', { name: 'Insert image' }).click();

	// Checking… while the worker verifies it (if it hasn't already), then the picture.
	const image = body(page).getByTestId('rich-image');
	await expect(image).toHaveAttribute('data-state', /quarantined|usable/);
	await expect(image).toHaveAttribute('data-state', 'usable', { timeout: 20_000 });
	await expect(image.locator('img')).toHaveAttribute('src', /\/media\/v1\//);
	await expect
		.poll(async () => (await storedImages(page, project, entry)).map((image) => image.alt), { timeout: 10_000 })
		.toEqual(['A green field under an evening sky']);
});

test('a pasted image whose upload answer is lost is sent again with the same key: one upload, and it goes in (D-215)', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Lost answer');
	// The first upload reaches the server and is stored, but its answer is a 504 (the deadline ran out while
	// the commit was finishing, as on a slow runner); the second goes through as it is.
	const keys: string[] = [];
	await page.route('**/environments/production/assets', async (route) => {
		if (route.request().method() !== 'POST') return route.continue();
		keys.push(route.request().headers()['idempotency-key'] ?? '');
		if (keys.length === 1) {
			await route.fetch();
			return route.fulfill({ status: 504, json: { error: { code: 'deadline_exceeded', message: 'the server took too long to answer' } } });
		}
		return route.continue();
	});
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	const editor = body(page).locator('.tiptap');
	await editor.click();
	await editor.evaluate((element, data) => {
		const bytes = Uint8Array.from(atob(data), (letter) => letter.charCodeAt(0));
		const transfer = new DataTransfer();
		transfer.items.add(new File([bytes], 'lost.png', { type: 'image/png' }));
		element.dispatchEvent(new ClipboardEvent('paste', { clipboardData: transfer, bubbles: true, cancelable: true }));
	}, png(800, 450, [200, 90, 120]).toString('base64'));
	const describe = page.getByRole('dialog', { name: 'Describe the image' });
	await expect(describe.getByTestId('image-chosen')).toContainText('lost.png');
	await expect(describe.getByTestId('image-chosen')).not.toContainText('Uploading');
	await describe.getByLabel('Alt text').fill('A harbour wall at low tide');
	await describe.getByRole('button', { name: 'Insert image' }).click();
	await expect(describe).toBeHidden();
	await expect(body(page).getByTestId('rich-image')).toHaveCount(1);
	// Sent twice, with one key; stored once.
	expect(keys).toHaveLength(2);
	expect(keys[1]).toBe(keys[0]);
	const listed = await api(page, 'GET', `${env(project)}/assets`);
	expect(listed.body?.['data'], JSON.stringify(listed.body)).toHaveLength(1);
});
