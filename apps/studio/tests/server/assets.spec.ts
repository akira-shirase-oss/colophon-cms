// Uploads and the media picker (media M.2, D-095) in the Studio, against the real server and the
// real object store: an image field built in the builder; a file uploaded through the picker,
// checked by the worker, and chosen at once; the library lists the project's uploads with their
// states; a list of two is picked, reordered, and trimmed; a file that isn't an image is reported
// rejected and never chosen. Choices survive a reload — and a thumbnail comes from the media
// origin for a usable upload only.

import { expect, type Page, test } from '@playwright/test';
import { api, env, key, newEntry, newProject, signIn } from './helpers.ts';

// A 1×1 transparent PNG — a real image the worker's verifier accepts.
const PNG = 'iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJAAAADUlEQVR42mNk+M9QDwADhgGAWjR9awAAAABJRU5ErkJggg==';
const png = Buffer.from(PNG, 'base64');

const GALLERIES = {
	name: 'Galleries',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'photo', label: 'Photo', type: 'asset', required: false, many: false },
		{ id: 'photos', label: 'Photos', type: 'asset', required: false, many: true, max: 2 }
	]
};

const status = (page: Page) => page.getByTestId('save-status');

/** A project with the galleries type and one entry; the entry's editor opened. */
async function gallery(page: Page): Promise<{ project: string; entry: string }> {
	await signIn(page);
	const project = await newProject(page);
	const made = await api(page, 'POST', `${env(project)}/content-types`, { slug: 'galleries', definition: GALLERIES }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const entry = await newEntry(page, project, 'First gallery', 'galleries', { title: 'First gallery' });
	await page.goto(`/studio/projects/${project}/galleries/${entry}`);
	return { project, entry };
}

/** An upload through the API (the raw-bytes endpoint, not the picker): its id once the worker
 *  has made it usable. */
async function uploaded(page: Page, project: string, name: string): Promise<string> {
	const made = await page.evaluate(
		async ({ project, name, png }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(`/api/v1/projects/${project}/environments/production/assets`, {
				method: 'POST',
				headers: { 'Content-Type': 'image/png', 'X-CSRF-Token': csrf, 'X-Colophon-File-Name': name, 'Idempotency-Key': crypto.randomUUID() },
				body: Uint8Array.from(atob(png), (letter) => letter.charCodeAt(0))
			});
			return { status: response.status, id: ((await response.json().catch(() => null))?.data?.id as string) ?? '' };
		},
		{ project, name, png: PNG }
	);
	expect(made.status).toBe(202);
	for (let attempt = 0; attempt < 50; attempt += 1) {
		const read = await api(page, 'GET', `${env(project)}/assets/${made.id}`);
		if (read.body?.['data']?.state === 'usable') return made.id;
		await page.waitForTimeout(200);
	}
	throw new Error('the upload never settled');
}

test('a photo is uploaded through the picker, checked, chosen, and kept after a reload', async ({ page }) => {
	const { entry, project } = await gallery(page);
	const photo = page.getByTestId('asset-photo');
	await photo.getByTestId('asset-toggle').click();
	await expect(photo.getByTestId('asset-library')).toContainText('No uploads yet');
	await photo.getByTestId('asset-file').setInputFiles({ name: 'cover.png', mimeType: 'image/png', buffer: png });
	// The picker waits on the worker, then chooses the usable upload itself.
	await expect(photo.getByTestId('asset-chip')).toContainText('cover.png', { timeout: 20_000 });
	await expect(photo.getByTestId('asset-chip').locator('img')).toHaveAttribute('src', new RegExp(`/media/v1/${project}/production/.+`));
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	// A file that isn't an image is reported and never chosen — on the empty list field, since
	// a full single field keeps the library closed.
	const photos = page.getByTestId('asset-photos');
	await photos.getByTestId('asset-toggle').click();
	await photos.getByTestId('asset-file').setInputFiles({ name: 'fake.png', mimeType: 'image/png', buffer: Buffer.from('this is not an image') });
	await expect(photos.getByTestId('asset-problem')).toContainText('rejected', { timeout: 20_000 });
	await expect(photos.getByTestId('asset-chip')).toHaveCount(0);

	await page.reload();
	await expect(photo.getByTestId('asset-chip')).toContainText('cover.png');
});

test('a list of two is picked from the library, reordered, trimmed, and kept', async ({ page }) => {
	const { project, entry } = await gallery(page);
	const first = await uploaded(page, project, 'one.png');
	const second = await uploaded(page, project, 'two.png');
	await page.reload();

	const photos = page.getByTestId('asset-photos');
	await photos.getByTestId('asset-toggle').click();
	const options = photos.getByTestId('asset-option');
	await expect(options).toHaveCount(2);
	await options.filter({ hasText: 'one.png' }).click();
	await options.filter({ hasText: 'two.png' }).click();
	await expect(photos.getByTestId('asset-chip')).toHaveCount(2);
	// At the bound, the library is closed to new choices.
	await expect(photos.getByTestId('asset-toggle')).toHaveCount(0);
	await expect(photos.getByText('At most 2 images')).toBeVisible();

	await photos.getByRole('button', { name: 'Move two.png up' }).click();
	await expect(photos.getByTestId('asset-chip').first()).toContainText('two.png');
	await photos.getByRole('button', { name: 'Remove one.png' }).click();
	await expect(photos.getByTestId('asset-chip')).toHaveCount(1);

	// The saved value names the remaining upload — polled, since the autosave flushes on its own
	// clock and "saved" was already showing when the list was emptied.
	await expect
		.poll(async () => (await api(page, 'GET', `${env(project)}/collections/galleries/entries/${entry}`)).body?.['data'].content.photos, { timeout: 10_000 })
		.toEqual([second]);
});
