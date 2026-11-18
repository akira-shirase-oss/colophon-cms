// F10 (UI audit, low confidence): content lost under instant typing mixed with toolbar commands.
// A seeded run types words with no delay, interleaving Enter, Bold, the Heading menu and the
// Callout menu, while every save takes 1.5 s, then checks that every word typed is in the editor
// and in the saved draft, once each and in order. Several seeds, so a loss that depends on where
// a save lands is likely to show; a failure prints its seed and steps to replay it.

import { expect, type Page, test } from '@playwright/test';
import { api, env, newEntry, newProject, signIn } from './helpers.ts';

/** A small seeded generator (mulberry32): the same seed, the same run. */
function random(seed: number): () => number {
	let state = seed >>> 0;
	return () => {
		state = (state + 0x6d2b79f5) >>> 0;
		let t = state;
		t = Math.imul(t ^ (t >>> 15), t | 1);
		t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
		return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
	};
}

/** Every text node's text in a stored rich-text document, in order. */
function texts(node: unknown, out: string[] = []): string[] {
	if (node !== null && typeof node === 'object') {
		const { text, content } = node as { text?: unknown; content?: unknown };
		if (typeof text === 'string') out.push(text);
		if (Array.isArray(content)) for (const child of content) texts(child, out);
	}
	return out;
}

async function command(page: Page, step: number) {
	switch (step) {
		case 0:
			await page.keyboard.press('Enter');
			break;
		case 1:
			await page.getByRole('button', { name: 'Bold' }).click();
			break;
		case 2:
			await page.getByTestId('heading-menu').click();
			await page.getByRole('menuitem', { name: /Heading 2/ }).click();
			break;
		case 3:
			await page.getByTestId('callout-menu').click();
			await page.getByRole('menuitem').first().click();
			break;
		default:
			await page.keyboard.press('Enter');
			await page.getByTestId('heading-menu').click();
			await page.getByRole('menuitem', { name: 'Normal text' }).click();
	}
}

for (const seed of [11, 42, 2026]) {
	test(`fast typing with toolbar commands while saves are slow loses nothing (seed ${seed})`, async ({ page }) => {
		test.setTimeout(120_000);
		await signIn(page);
		const project = await newProject(page);
		const entry = await newEntry(page, project, 'Fuzz');
		await page.route('**/collections/articles/entries/*', async (route) => {
			if (route.request().method() === 'PUT') await new Promise((wake) => setTimeout(wake, 1500));
			await route.continue();
		});
		await page.goto(`/studio/projects/${project}/articles/${entry}`);
		const body = page.getByRole('textbox', { name: 'Body' });
		await body.click();
		const next = random(seed);
		const words: string[] = [];
		const steps: string[] = [];
		for (let i = 0; i < 30; i += 1) {
			const word = `w${seed}x${i}q`;
			words.push(word);
			await page.keyboard.type(`${word} `, { delay: 0 });
			steps.push(`type ${word}`);
			if (next() < 0.45) {
				const which = Math.floor(next() * 5);
				steps.push(`command ${which}`);
				await command(page, which);
				// Type on once the caret is back in the text, as a person does: keys pressed while
				// a menu still holds the focus belong to the menu, not to the text.
				await expect(body).toBeFocused();
			}
		}
		await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved', { timeout: 60_000 });
		const shown = (await body.innerText()).replace(/\s+/g, ' ');
		const stored = await api(page, 'GET', `${env(project)}/collections/articles/entries/${entry}`);
		const saved = texts(stored.body?.['data'].content.body?.doc).join(' ').replace(/\s+/g, ' ');
		const trail = `seed ${seed}: ${steps.join(', ')}\nscreen: ${shown}\nsaved: ${saved}`;
		let shownAt = -1;
		let savedAt = -1;
		for (const word of words) {
			const inShown = shown.indexOf(word, shownAt + 1);
			const inSaved = saved.indexOf(word, savedAt + 1);
			expect(inShown, `${word} missing on screen — ${trail}`).toBeGreaterThan(shownAt);
			expect(inSaved, `${word} missing from the saved draft — ${trail}`).toBeGreaterThan(savedAt);
			shownAt = inShown;
			savedAt = inSaved;
			expect(saved.split(word).length - 1, `${word} saved more than once — ${trail}`).toBe(1);
		}
	});
}
