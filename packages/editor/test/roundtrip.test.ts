import { describe, expect, test } from 'vitest';
import { LIMITS, measure, openEditor, serialize, toStored, UnrepresentableContent } from '../src/index.ts';
import { invalid, valid } from './fixtures.ts';

describe('every valid fixture survives the editor byte for byte', () => {
	test.each(valid.map((fixture) => [fixture.name, fixture] as const))('%s', (_name, fixture) => {
		const editor = openEditor(fixture.value);
		const once = toStored(editor.getJSON());
		editor.destroy();
		expect(serialize(once)).toBe(serialize(fixture.value));

		// And again: the canonical form is a fixed point.
		const again = openEditor(once);
		expect(serialize(toStored(again.getJSON()))).toBe(serialize(fixture.value));
		again.destroy();
	});
});

describe('every invalid fixture is refused by the editor or kept intact, never trimmed', () => {
	test.each(invalid.map((fixture) => [fixture.name, fixture] as const))('%s', (_name, fixture) => {
		let editor;
		try {
			editor = openEditor(fixture.value);
		} catch (error) {
			expect(error).toBeInstanceOf(UnrepresentableContent);
			return;
		}
		// Opened: then nothing may have been lost. The Rust validator still refuses it (step 0.5).
		expect(serialize(toStored(editor.getJSON()))).toBe(serialize(fixture.value));
		editor.destroy();
	});
});

test('the fixtures cover every node and mark in the format', () => {
	const nodes = new Set<string>();
	const marks = new Set<string>();
	const walk = (node: { type: string; marks?: { type: string }[]; content?: unknown[] }) => {
		nodes.add(node.type);
		for (const mark of node.marks ?? []) marks.add(mark.type);
		for (const child of node.content ?? []) walk(child as typeof node);
	};
	for (const fixture of valid) walk(fixture.value.doc);
	expect([...nodes].sort()).toEqual(
		['blockquote', 'bulletList', 'callout', 'codeBlock', 'doc', 'hardBreak', 'heading', 'horizontalRule', 'image', 'listItem', 'orderedList', 'paragraph', 'text'].sort()
	);
	expect([...marks].sort()).toEqual(['bold', 'code', 'italic', 'link', 'strike']);
});

test('the depth fixture sits exactly at the limit', () => {
	const fixture = valid.find((candidate) => candidate.name === '11-depth-at-the-limit');
	expect(fixture && measure(fixture.value).depth).toBe(LIMITS.maxDepth);
});
