// The seven field types of step M.1 (D-064) in the Studio, against the real server: built in the
// content-type builder with their options and help text; edited with fitting controls; saved and
// read back in the server's canonical form (a web address as `new URL(text).href`, a colour in
// lowercase, JSON with its keys sorted), with nothing left to save after a reload; invalid values
// refused in plain words; and the Studio's JSON canonical form equal to the server's stored bytes.

import { expect, type Page, test } from '@playwright/test';
import { canonicalJson } from '../../src/lib/json.ts';
import { TRICKY, TRICKY_CANONICAL } from '../json-fixture.ts';
import { api, env, internalTerms, key, newProject, shot, signIn, violations } from './helpers.ts';

/** A content type with one field of each new kind, made through the API. */
const VENUES = {
	name: 'Venues',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false, min_chars: 3 },
		{ id: 'contact_email', label: 'Contact email', type: 'email', required: false },
		{ id: 'website', label: 'Website', type: 'url', required: false, description: 'Where readers book a table' },
		{ id: 'opens_on', label: 'Opens on', type: 'date', required: false },
		{ id: 'opens_at', label: 'Opens at', type: 'time', required: false },
		{ id: 'settings', label: 'Settings', type: 'json', required: false },
		{ id: 'notes', label: 'Notes', type: 'markdown', required: false, max_chars: 60 },
		{ id: 'accent', label: 'Accent', type: 'color', required: false }
	]
};

async function venue(page: Page): Promise<{ project: string; entry: string }> {
	await signIn(page);
	const project = await newProject(page);
	const made = await api(page, 'POST', `${env(project)}/content-types`, { slug: 'venues', definition: VENUES }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	await page.goto(`/studio/projects/${project}/venues?new=1`);
	await page.getByLabel('Title', { exact: true }).fill('Harbour hall');
	await page.getByRole('button', { name: 'Create', exact: true }).click();
	await expect(page.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	return { project, entry: new URL(page.url()).pathname.split('/').at(-1) ?? '' };
}

/** The stored content of an entry, as the server sends it: its exact bytes. */
async function stored(page: Page, project: string, entry: string): Promise<string> {
	return page.evaluate(async (path) => (await fetch(path, { cache: 'no-store' })).text(), `${env(project)}/collections/venues/entries/${entry}`);
}

/** The bytes of one top-level field's value in `content`'s JSON text. */
function valueAt(text: string, field: string): string {
	const marker = `${JSON.stringify(field)}:`;
	const start = text.indexOf(marker, text.indexOf('"content":')) + marker.length;
	let depth = 0;
	let inString = false;
	let at = start;
	for (; at < text.length; at += 1) {
		const character = text[at];
		if (inString) {
			if (character === '\\') at += 1;
			else if (character === '"') inString = false;
			continue;
		}
		if (character === '"') inString = true;
		else if (character === '{' || character === '[') depth += 1;
		else if (character === '}' || character === ']') {
			if (depth === 0) break;
			depth -= 1;
			if (depth === 0) {
				at += 1;
				break;
			}
		} else if (character === ',' && depth === 0) break;
	}
	return text.slice(start, at);
}

const status = (page: Page) => page.getByTestId('save-status');

test('an owner builds a content type with the seven new kinds of field, help text and limits', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	await page.goto(`/studio/projects/${project}/settings`);
	await page.locator('#type-name').fill('Venues');
	await page.getByRole('button', { name: 'Create and add fields' }).click();
	await expect(page.getByTestId('field-title')).toBeVisible();
	const kinds = page.getByLabel('Type');
	const offered = (await kinds.locator('option').allTextContents()).map((name) => name.trim());
	for (const name of ['Email address', 'Web address', 'Date', 'Time of day', 'Structured data (JSON)', 'Markdown text', 'Colour']) {
		expect(offered, name).toContain(name);
	}
	const add = async (label: string, type: string) => {
		await page.getByLabel('New field').fill(label);
		await kinds.selectOption(type);
		await page.getByRole('button', { name: 'Add field' }).click();
		await expect(page.getByTestId(`field-${label.toLowerCase().replaceAll(' ', '_')}`)).toBeVisible();
	};
	await add('Contact email', 'email');
	await add('Website', 'url');
	await add('Opens on', 'date');
	await add('Opens at', 'time');
	await add('Settings', 'json');
	await add('Notes', 'markdown');
	await add('Accent', 'color');
	await page.getByTestId('field-title').getByLabel('At least').fill('3');
	await page.getByTestId('field-notes').getByLabel('Up to').fill('60');
	await page.getByTestId('field-website').getByTestId('field-description').fill('Where readers book a table ');
	await expect(page.getByTestId('field-settings')).toContainText('Structured data (JSON)');
	expect(await violations(page)).toEqual([]);
	await page.getByRole('button', { name: 'Save content type' }).click();
	await expect(page.getByText('Saved Venues, version 2')).toBeVisible();
	await shot(page, 'builder-m1');

	const saved = await api(page, 'GET', `${env(project)}/content-types/venues`);
	const fields = (saved.body?.['data'].definition.fields ?? []) as Record<string, unknown>[];
	expect(fields.map((field) => field['type'])).toEqual(['text', 'email', 'url', 'date', 'time', 'json', 'markdown', 'color']);
	expect(fields.find((field) => field['id'] === 'title')).toMatchObject({ min_chars: 3 });
	expect(fields.find((field) => field['id'] === 'notes')).toMatchObject({ max_chars: 60 });
	expect(fields.find((field) => field['id'] === 'website')).toMatchObject({ description: 'Where readers book a table' });
	expect(fields.find((field) => field['id'] === 'accent')).not.toHaveProperty('description');
});

test('each field gets a fitting control, and saved values come back in the server’s canonical form', async ({ page }) => {
	const { project, entry } = await venue(page);
	await expect(page.getByText('Where readers book a table')).toBeVisible();
	await expect(page.getByLabel('Contact email (optional)')).toHaveAttribute('type', 'email');
	await expect(page.getByLabel('Website (optional)')).toHaveAttribute('type', 'url');
	await expect(page.getByLabel('Opens on (optional)')).toHaveAttribute('type', 'date');
	await expect(page.getByLabel('Opens at (optional)')).toHaveAttribute('type', 'time');
	await expect(page.getByLabel('Pick Accent')).toHaveAttribute('type', 'color');

	await page.getByLabel('Contact email (optional)').fill('ada@example.com');
	await page.getByLabel('Website (optional)').fill('HTTPS://Example.COM');
	await page.getByLabel('Website (optional)').blur();
	await expect(page.getByLabel('Website (optional)')).toHaveValue('https://example.com/');
	await page.getByLabel('Opens on (optional)').fill('2026-10-28');
	await page.getByLabel('Opens at (optional)').fill('14:30');
	await page.getByLabel('Settings (optional)').fill('{"b": 1, "a": {"y": 1.5, "x": -0}}');
	await page.getByLabel('Notes (optional)').fill('Hello *world*');
	await expect(page.getByTestId('markdown-count')).toHaveText('13 of 60 characters');
	await page.getByLabel('Accent (optional)').fill('#1A2B3C');
	await expect(page.getByLabel('Pick Accent')).toHaveValue('#1a2b3c');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	await expect(page.getByTestId('field-problem')).toHaveCount(0);
	expect(await violations(page)).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await shot(page, 'editor-m1');

	const text = await stored(page, project, entry);
	expect(valueAt(text, 'contact_email')).toBe('"ada@example.com"');
	expect(valueAt(text, 'website')).toBe('"https://example.com/"');
	expect(valueAt(text, 'opens_on')).toBe('"2026-10-28"');
	expect(valueAt(text, 'opens_at')).toBe('"14:30"');
	expect(valueAt(text, 'settings')).toBe('{"a":{"x":0,"y":1.5},"b":1}');
	expect(valueAt(text, 'notes')).toBe('"Hello *world*"');
	expect(valueAt(text, 'accent')).toBe('"#1a2b3c"');

	// Reopened, the editor shows the stored forms and finds nothing to save.
	const saves: string[] = [];
	page.on('request', (request) => {
		if (request.method() === 'PUT' && request.url().includes('/entries/')) saves.push(request.url());
	});
	await page.reload();
	await expect(page.getByLabel('Settings (optional)')).toHaveValue('{\n  "a": {\n    "x": 0,\n    "y": 1.5\n  },\n  "b": 1\n}');
	await expect(page.getByLabel('Accent (optional)')).toHaveValue('#1a2b3c');
	await page.waitForTimeout(2_000);
	await expect(status(page)).toHaveAttribute('data-status', 'saved');
	expect(saves).toEqual([]);

	// On a phone, every control fits.
	await page.setViewportSize({ width: 390, height: 844 });
	await page.waitForLoadState('networkidle');
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
	expect(await violations(page)).toEqual([]);
});

test('invalid values are caught as they are typed, refused by the server, and said plainly', async ({ page }) => {
	await venue(page);
	const problem = (field: string) => page.locator(`[data-testid="field-problem"][data-field="${field}"]`);

	// JSON that doesn't read is never sent: the save waits, and says why.
	const saves: string[] = [];
	page.on('request', (request) => {
		if (request.method() === 'PUT' && request.url().includes('/entries/')) saves.push(request.url());
	});
	await page.getByLabel('Settings (optional)').fill('{"a":');
	await expect(status(page)).toHaveAttribute('data-status', 'held');
	await expect(status(page)).toContainText("Not saved: Settings: This isn't valid JSON yet");
	await expect(problem('settings')).toContainText("This isn't valid JSON yet");
	await page.waitForTimeout(1_500);
	expect(saves).toEqual([]);
	expect(await internalTerms(page)).toEqual([]);
	await page.getByLabel('Settings (optional)').fill('{"a": 1}');
	await expect(status(page)).toHaveAttribute('data-status', 'saved');

	// The others are checked here as typed, and the server refuses them in the same words.
	for (const [label, field, value, words] of [
		['Contact email (optional)', 'contact_email', 'ada@localhost', 'Contact email is an email address like name@example.com.'],
		['Website (optional)', 'website', 'ftp://example.com', 'Website is a web address starting with https:// or http://.'],
		['Accent (optional)', 'accent', '#123', 'Accent is a colour like #1a2b3c'],
		['Title', 'title', 'Hi', 'Title is at least 3 characters.']
	] as const) {
		await page.getByLabel(label, { exact: true }).fill(value);
		await expect(problem(field)).toContainText(words);
		await expect(status(page)).toHaveAttribute('data-status', 'refused');
		await expect(status(page)).toContainText(`Not saved: ${words}`);
		expect(await internalTerms(page), field).toEqual([]);
		await page.getByLabel(label, { exact: true }).fill(field === 'title' ? 'Harbour hall' : '');
		await expect(status(page)).toHaveAttribute('data-status', 'saved');
	}
	expect(await violations(page)).toEqual([]);
});

test('the Studio’s JSON canonical form is the server’s, byte for byte', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'venues', definition: VENUES }, { 'Idempotency-Key': key() })).status).toBe(201);
	const made = await api(page, 'POST', `${env(project)}/collections/venues/entries`, { title: 'Tricky', settings: TRICKY }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const kept = valueAt(await stored(page, project, made.body?.['data'].id as string), 'settings');
	expect(kept).toBe(canonicalJson(TRICKY));
	expect(kept).toBe(TRICKY_CANONICAL);
});
