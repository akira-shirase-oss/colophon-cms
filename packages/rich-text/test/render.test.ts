// The built package (dist/, what npm would ship) against the shared fixtures: every valid
// document renders to its reviewed HTML in fixtures/rich-text/v1/rendered — the same bytes the
// content renderer's own suite pins, so the two can't drift — and every invalid one is refused
// or rendered without anything unsafe. scripts/pack-check.mjs runs the same comparison once more
// against the packed tarball installed in a scratch project.

import { readdirSync, readFileSync } from 'node:fs';
import { describe, expect, test } from 'vitest';
import { renderToHtml, RENDERER_VERSION, RICH_TEXT_SCHEMA, RICH_TEXT_VERSION, UnrenderableContent, type RichTextValue } from '../dist/index.js';

const root = new URL('../../../fixtures/rich-text/v1/', import.meta.url);
const options = { assetUrl: (id: string) => `https://assets.example/${id}` };

function load(kind: 'valid' | 'invalid'): [string, RichTextValue][] {
	return readdirSync(new URL(kind, root))
		.filter((file) => file.endsWith('.json'))
		.sort()
		.map((file) => [file.replace(/\.json$/, ''), (JSON.parse(readFileSync(new URL(`${kind}/${file}`, root), 'utf8')) as { value: RichTextValue }).value]);
}

describe('the built renderer', () => {
	test('names the format and its own version', () => {
		expect(RICH_TEXT_SCHEMA).toBe('colophon/rich-text');
		expect(RICH_TEXT_VERSION).toBe(1);
		expect(Number.isInteger(RENDERER_VERSION)).toBe(true);
	});

	const valid = load('valid');
	test('has fixtures to render', () => expect(valid.length).toBeGreaterThan(5));
	test.each(valid)('renders %s to its reviewed output', (name, value) => {
		const reviewed = readFileSync(new URL(`rendered/${name}.html`, root), 'utf8');
		expect(renderToHtml(value, options) + '\n').toBe(reviewed);
	});

	test.each(load('invalid'))('refuses %s, or renders it without anything unsafe', (_name, value) => {
		let html: string;
		try {
			html = renderToHtml(value, options);
		} catch (error) {
			expect(error).toBeInstanceOf(UnrenderableContent);
			return;
		}
		for (const forbidden of ['<script', 'onclick', 'style=', 'evil.example', 'javascript:', 'data:', 'iframe']) {
			expect(html, forbidden).not.toContain(forbidden);
		}
	});
});
