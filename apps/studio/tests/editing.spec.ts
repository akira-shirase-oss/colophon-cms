// Writing in a real browser: keyboard, clipboard, toolbar.

import { expect, test } from '@playwright/test';
import { freshEditor, stored, storedDoc } from './helpers.ts';

test('keyboard shortcuts format text', async ({ page }) => {
	await freshEditor(page);
	await page.keyboard.type('Hello ');
	await page.keyboard.press('ControlOrMeta+b');
	await page.keyboard.type('bold');
	await page.keyboard.press('ControlOrMeta+b');
	await page.keyboard.type(' and ');
	await page.keyboard.press('ControlOrMeta+i');
	await page.keyboard.type('italic');
	expect(await storedDoc(page)).toEqual({
		type: 'doc',
		content: [
			{
				type: 'paragraph',
				content: [
					{ type: 'text', text: 'Hello ' },
					{ type: 'text', text: 'bold', marks: [{ type: 'bold' }] },
					{ type: 'text', text: ' and ' },
					{ type: 'text', text: 'italic', marks: [{ type: 'italic' }] }
				]
			}
		]
	});
});

test('markdown shortcuts make headings, lists and quotes, but never a level-1 heading', async ({ page }) => {
	await freshEditor(page);
	await page.keyboard.type('## Section');
	await page.keyboard.press('Enter');
	await page.keyboard.type('- item');
	await page.keyboard.press('Enter');
	await page.keyboard.press('Enter');
	await page.keyboard.type('# not a title');
	const json = await stored(page);
	expect(json).toContain('{"type":"heading","attrs":{"level":2},"content":[{"type":"text","text":"Section"}]}');
	expect(json).toContain('{"type":"bulletList","content":[{"type":"listItem","content":[{"type":"paragraph","content":[{"type":"text","text":"item"}]}]}]}');
	expect(json).toContain('{"type":"paragraph","content":[{"type":"text","text":"# not a title"}]}');
	expect(json).not.toContain('"level":1');
});

test('a real paste keeps the words and drops scripts, styles, frames, foreign images and unsafe links', async ({ page, context }) => {
	await context.grantPermissions(['clipboard-read', 'clipboard-write']);
	await freshEditor(page);
	const html =
		'<h1 style="color:red">Pasted title</h1>' +
		'<p onclick="steal()" style="font-size:40px">Hello <b>bold</b> <span style="color:red">red</span></p>' +
		'<script>alert(1)</script><iframe src="https://evil.example"></iframe>' +
		'<p><img src="https://evil.example/pixel.gif" onerror="alert(1)">after image</p>' +
		'<p><a href="javascript:alert(1)">bad link</a> <a href="https://example.com">good link</a></p>';
	await page.evaluate(async (markup) => {
		await navigator.clipboard.write([
			new ClipboardItem({
				'text/html': new Blob([markup], { type: 'text/html' }),
				'text/plain': new Blob(['fallback'], { type: 'text/plain' })
			})
		]);
	}, html);
	await page.keyboard.press('ControlOrMeta+v');
	const json = await stored(page);
	expect(json).toContain('Pasted title');
	expect(json).toContain('{"type":"text","text":"bold","marks":[{"type":"bold"}]}');
	// Chrome sanitizes HTML on the async clipboard before the editor sees it: it already drops the
	// javascript: href and normalizes the URL with a trailing slash. So this test can't prove the
	// editor's own filtering; packages/editor/test/editing.test.ts pastes unsanitized HTML through
	// ProseMirror directly for that.
	expect(json).toMatch(/"href":"https:\/\/example\.com\/?"/);
	for (const forbidden of ['script', 'iframe', 'style', 'onclick', 'onerror', 'evil.example', 'javascript:', '"image"', '"level":1']) {
		expect(json, forbidden).not.toContain(forbidden);
	}
});

test('keyboard undo and redo', async ({ page }) => {
	await freshEditor(page);
	await page.keyboard.type('first');
	// Typing within half a second is one undo step; pause so the next words are a new one.
	await page.waitForTimeout(700);
	await page.keyboard.type(' second');
	await page.keyboard.press('ControlOrMeta+z');
	expect(await stored(page)).toContain('"text":"first"');
	expect(await stored(page)).not.toContain('second');
	await page.keyboard.press('ControlOrMeta+Shift+z');
	expect(await stored(page)).toContain('"text":"first second"');
});

test('the callout buttons wrap and unwrap a paragraph', async ({ page }) => {
	await freshEditor(page);
	await page.keyboard.type('Back up first');
	await page.getByRole('button', { name: 'Warning' }).click();
	expect(await storedDoc(page)).toEqual({
		type: 'doc',
		content: [
			{
				type: 'callout',
				attrs: { tone: 'warning' },
				content: [{ type: 'paragraph', content: [{ type: 'text', text: 'Back up first' }] }]
			}
		]
	});
	await page.getByRole('button', { name: 'Warning' }).click();
	expect(await stored(page)).not.toContain('callout');
});

test('an image needs alt text and gets a media-library id, never a URL', async ({ page }) => {
	await freshEditor(page);
	await page.getByRole('button', { name: 'Image', exact: true }).click();
	const insert = page.getByRole('button', { name: 'Insert image' });
	await expect(insert).toBeDisabled();
	await page.getByLabel('Alt text').fill('   ');
	await expect(insert).toBeDisabled();
	await page.getByLabel('Alt text').fill('The octagonal C mark');
	await insert.click();
	const image = JSON.stringify(await storedDoc(page));
	expect(image).toMatch(/\{"type":"image","attrs":\{"assetId":"[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}","alt":"The octagonal C mark"\}\}/);
	expect(image).not.toContain('http');
});

test('an unsafe link is refused with an explanation; a safe one is applied', async ({ page }) => {
	await freshEditor(page);
	await page.keyboard.type('click me');
	await page.keyboard.press('ControlOrMeta+a');
	await page.getByRole('button', { name: 'Link' }).click();
	await page.getByLabel('Link').fill('javascript:alert(1)');
	await page.getByRole('button', { name: 'Apply' }).click();
	await expect(page.getByRole('alert')).toContainText('Only https:, http: and mailto: links');
	expect(await stored(page)).not.toContain('"link"');
	await page.getByLabel('Link').fill('https://example.com/docs');
	await page.getByRole('button', { name: 'Apply' }).click();
	expect(await stored(page)).toContain('"marks":[{"type":"link","attrs":{"href":"https://example.com/docs"}}]');
});

test('the limits line reflects the document', async ({ page }) => {
	await freshEditor(page);
	await expect(page.getByTestId('limits')).toContainText('Depth 2/16');
	await page.keyboard.type('x');
	await expect(page.getByTestId('limits')).toContainText('Depth 3/16');
});
