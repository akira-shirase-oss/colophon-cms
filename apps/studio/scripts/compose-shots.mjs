// Side-by-side before|after composites of the Studio's screenshots (the UI audit's record, D-185):
// `node scripts/compose-shots.mjs <before-dir> <after-dir> <out-dir>` pairs every
// `<screen>-<width>-<scheme>.png` present in both and writes `<screen>-<width>-<scheme>.png` to
// out-dir: "Before" on the left, "After" on the right, each at its own size, on a neutral ground.
// Playwright renders the pair, so no image library is needed.

import { existsSync, mkdirSync, readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { chromium } from '@playwright/test';

const [before, after, out] = process.argv.slice(2);
if (!before || !after || !out) {
	console.error('usage: node scripts/compose-shots.mjs <before-dir> <after-dir> <out-dir>');
	process.exit(2);
}
mkdirSync(out, { recursive: true });
const names = readdirSync(after)
	.filter((name) => name.endsWith('.png') && existsSync(join(before, name)))
	.sort();
const browser = await chromium.launch();
const page = await browser.newPage({ deviceScaleFactor: 1 });
for (const name of names) {
	const uri = (dir) => `data:image/png;base64,${readFileSync(join(dir, name)).toString('base64')}`;
	const dark = name.includes('-dark');
	await page.setContent(`<!doctype html><html><body style="margin:0;background:${dark ? '#1b1b1f' : '#ececef'};font:600 20px system-ui,sans-serif;color:${dark ? '#e8e8ec' : '#1b1b1f'}">
<div id="pair" style="display:inline-flex;gap:32px;padding:24px;align-items:flex-start">
<figure style="margin:0"><figcaption style="margin:0 0 12px">Before</figcaption><img src="${uri(before)}" style="display:block;outline:1px solid #8888"></figure>
<figure style="margin:0"><figcaption style="margin:0 0 12px">After</figcaption><img src="${uri(after)}" style="display:block;outline:1px solid #8888"></figure>
</div></body></html>`);
	await page.waitForFunction(() => [...document.images].every((image) => image.complete));
	await page.locator('#pair').screenshot({ path: join(out, name) });
	console.log(`composed ${name}`);
}
await browser.close();
console.log(`${names.length} composites in ${out}`);
