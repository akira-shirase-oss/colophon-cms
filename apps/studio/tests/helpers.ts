import { readdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, type Page } from '@playwright/test';

const root = join(import.meta.dirname, '../../../fixtures/rich-text/v1');

export interface FixtureFile {
	name: string;
	value: unknown;
}

export function fixtures(kind: 'valid' | 'invalid'): FixtureFile[] {
	return readdirSync(join(root, kind))
		.filter((file) => file.endsWith('.json'))
		.sort()
		.map((file) => ({
			name: file.replace(/\.json$/, ''),
			value: JSON.parse(readFileSync(join(root, kind, file), 'utf8')).value
		}));
}

/** The canonical stored JSON the page shows. */
export async function stored(page: Page): Promise<string> {
	return (await page.getByTestId('stored').getAttribute('data-json')) ?? '';
}

export async function storedDoc(page: Page): Promise<unknown> {
	return JSON.parse(await stored(page)).doc;
}

export async function openFixture(page: Page, id: string) {
	await page.getByTestId('fixture-select').selectOption(id);
	await page.getByRole('button', { name: 'Open', exact: true }).click();
	await expect(page.getByTestId('open-status')).toContainText(id);
}

/** A fresh, empty document with the cursor in it. */
export async function freshEditor(page: Page) {
	await page.goto('/studio/playground');
	await expect(page.getByTestId('open-status')).toHaveAttribute('data-kind', 'opened');
	await page.locator('.tiptap').click();
}
