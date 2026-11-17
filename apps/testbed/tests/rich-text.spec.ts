import { readdirSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

const root = join(import.meta.dirname, '../../../fixtures/rich-text/v1');
const total = ['valid', 'invalid'].reduce(
	(sum, kind) => sum + readdirSync(join(root, kind)).filter((file) => file.endsWith('.json')).length,
	0
);

test('the server agrees with every shared rich-text fixture, through the real HTTP API', async ({ page }) => {
	await page.goto('/rich-text');
	await page.getByRole('button', { name: `Check all ${total} fixtures` }).click();
	await expect(page.getByTestId('summary')).toHaveText(`${total} of ${total} agree`, { timeout: 30_000 });
	await expect(page.locator('[data-result="fail"]')).toHaveCount(0);
});

test('a document typed by hand gets the server verdict', async ({ page }) => {
	await page.goto('/rich-text');
	await page.getByLabel('Rich-text document').fill('{"schema":"colophon/rich-text","version":1,"doc":{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"x","marks":[{"type":"link","attrs":{"href":"javascript:alert(1)"}}]}]}]}}');
	await page.getByRole('button', { name: 'Validate' }).click();
	await expect(page.getByTestId('try-status')).toContainText('HTTP 422');
	await expect(page.locator('pre')).toContainText('"reason":"unsafe_link"');
});

// Images inside rich text (gap 3): a real upload named by an image node is a tracked asset
// reference — unknown ids refused at their place, deletion refused while a draft uses it — and
// the release records the version it published, which delivery hands back with its size and
// renditions and the versioned address serves for a year.
test('an image inside rich text is a tracked upload, delivered at the version its release published', async ({ page }) => {
	await signIn(page, 'dev-owner');
	const project = await page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const created = await fetch('/server/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Rich-text images' })
		});
		return ((await created.json()) as { data: { id: string } }).data.id;
	}, `tb-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
	await page.goto('/rich-text');
	await page.getByTestId('images-project').selectOption(project);
	await page.getByTestId('images-run').click();

	const steps = page.getByTestId('images-steps');
	await expect(steps).toContainText('served:', { timeout: 60_000 });
	await expect(steps).toContainText('upload: usable 400×4');
	await expect(steps.getByText(/^unknown image:/)).toContainText('422');
	await expect(steps.getByText(/^unknown image:/)).toContainText('asset_not_found at /body/doc/content/0/attrs/assetId');
	await expect(steps).toContainText('saved: 201');
	await expect(steps).toContainText('used in: body (draft)');
	await expect(steps.getByText(/^delete while used:/)).toContainText('409');
	await expect(steps.getByText(/^delete while used:/)).toContainText('still_referenced');
	await expect(steps).toContainText('published: 201');
	await expect(steps).toContainText('delivered: versioned url, current url bare, image/png 400×4, renditions w320');
	await expect(steps).toContainText('served: 200 image/png public, max-age=31536000, immutable');

	// The page draws the delivered address: the browser loads the released bytes.
	const drawn = page.getByTestId('images-delivered');
	await expect(drawn).toHaveAttribute('width', '400');
	await expect.poll(() => drawn.evaluate((img: HTMLImageElement) => img.complete && img.naturalWidth)).toBe(400);
});
