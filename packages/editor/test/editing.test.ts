// What the editor lets a writer do, and what it refuses to create. Paste goes through
// ProseMirror's real paste pipeline (`view.pasteHTML`); Playwright repeats the key ones with a
// real clipboard in a real browser.

import type { Editor } from '@tiptap/core';
import { closeHistory } from '@tiptap/pm/history';
import { afterEach, describe, expect, test } from 'vitest';
import { LIMITS, measure, openEditor, type RichTextValue, toStored } from '../src/index.ts';
import { valid } from './fixtures.ts';

const empty: RichTextValue = { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } };
const ASSET = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d';

let editor: Editor | undefined;
afterEach(() => editor?.destroy());

function open(value: RichTextValue = empty): Editor {
	editor = openEditor(value);
	return editor;
}

function stored(): string {
	return JSON.stringify(toStored((editor as Editor).getJSON()).doc);
}

describe('paste keeps only what the format allows', () => {
	test('scripts, styles, frames, foreign images and unsafe links are removed', () => {
		const ed = open();
		ed.view.pasteHTML(
			'<h1 style="color:red">Title</h1>' +
				'<p onclick="steal()" style="font-size:40px">Hello <b>bold</b> <span style="color:red">red</span></p>' +
				'<script>alert(1)</script><iframe src="https://evil.example"></iframe>' +
				'<p><img src="https://evil.example/pixel.gif" onerror="alert(1)">after image</p>' +
				'<p><a href="javascript:alert(1)">bad link</a> <a href="https://example.com">good link</a></p>'
		);
		const json = stored();
		for (const forbidden of ['script', 'iframe', 'style', 'onclick', 'onerror', 'evil.example', 'javascript:', '"image"']) {
			expect(json, forbidden).not.toContain(forbidden);
		}
		expect(json).toContain('"href":"https://example.com"');
		expect(json).toContain('{"type":"text","text":"bold","marks":[{"type":"bold"}]}');
		// h1 is the entry title, never a body heading.
		expect(json).not.toContain('"level":1');
	});

	test('pasted Colophon content keeps callouts and images', () => {
		const ed = open();
		ed.view.pasteHTML(
			`<aside data-callout data-tone="warning"><p>Careful</p></aside><figure data-colophon-image data-asset-id="${ASSET}" data-alt="A diagram"></figure>`
		);
		expect(stored()).toContain('{"type":"callout","attrs":{"tone":"warning"}');
		expect(stored()).toContain(`{"type":"image","attrs":{"assetId":"${ASSET}","alt":"A diagram"}}`);
	});

	test('an unknown callout tone pasted in becomes a note', () => {
		const ed = open();
		ed.view.pasteHTML('<aside data-callout data-tone="danger"><p>x</p></aside>');
		expect(stored()).toContain('"tone":"note"');
	});
});

describe('the editor refuses to create what the format rejects', () => {
	test('unsafe links', () => {
		const ed = open();
		ed.commands.insertContent('click');
		ed.commands.selectAll();
		for (const href of ['javascript:alert(1)', 'data:text/html,x', '//evil.example', 'vbscript:x']) {
			ed.commands.setLink({ href });
		}
		expect(stored()).not.toContain('"link"');
		ed.commands.setLink({ href: '/about' });
		expect(stored()).toContain('"href":"/about"');
	});

	test('heading level 1', () => {
		const ed = open();
		ed.commands.insertContent('x');
		expect(ed.commands.toggleHeading({ level: 1 as 2 })).toBe(false);
		expect(stored()).not.toContain('heading');
	});

	test('images without a valid asset id or alt text', () => {
		const ed = open();
		expect(ed.commands.insertImage({ assetId: ASSET, alt: '   ' })).toBe(false);
		expect(ed.commands.insertImage({ assetId: 'https://evil.example/x.png', alt: 'x' })).toBe(false);
		expect(ed.commands.insertImage({ assetId: ASSET, alt: 'x'.repeat(LIMITS.maxAltLength + 1) })).toBe(false);
		expect(stored()).not.toContain('"image"');
		expect(ed.commands.insertImage({ assetId: ASSET, alt: '  A diagram  ' })).toBe(true);
		expect(stored()).toContain(`{"type":"image","attrs":{"assetId":"${ASSET}","alt":"A diagram"}}`);
	});

	test('nesting past depth 16', () => {
		const fixture = valid.find((candidate) => candidate.name === '11-depth-at-the-limit');
		const ed = open(fixture?.value);
		const before = stored();
		// The innermost paragraph is already at depth 15; wrapping it again would push the text to 17.
		ed.commands.setTextSelection(ed.state.doc.content.size - 2);
		ed.commands.wrapIn('blockquote');
		expect(stored()).toBe(before);
		expect(measure(toStored(ed.getJSON())).depth).toBe(LIMITS.maxDepth);
	});
});

describe('editing', () => {
	test('a callout wraps the current paragraph and can be removed again', () => {
		const ed = open();
		ed.commands.insertContent('Heads up');
		expect(ed.commands.setCallout('tip')).toBe(true);
		expect(stored()).toBe('{"type":"doc","content":[{"type":"callout","attrs":{"tone":"tip"},"content":[{"type":"paragraph","content":[{"type":"text","text":"Heads up"}]}]}]}');
		ed.commands.unsetCallout();
		expect(stored()).toBe('{"type":"doc","content":[{"type":"paragraph","content":[{"type":"text","text":"Heads up"}]}]}');
	});

	test('undo and redo walk back and forth exactly', () => {
		const ed = open();
		ed.commands.insertContent('one');
		const afterOne = stored();
		// Edits within half a second form one undo step, as typing does. Here the writer pauses.
		ed.view.dispatch(closeHistory(ed.state.tr));
		ed.commands.toggleBold();
		ed.commands.insertContent(' two');
		const afterTwo = stored();
		ed.commands.undo();
		expect(stored()).toBe(afterOne);
		ed.commands.redo();
		expect(stored()).toBe(afterTwo);
		ed.commands.undo();
		ed.commands.undo();
		expect(stored()).toBe(JSON.stringify(empty.doc));
	});

	test('an edit changes only what was edited, even in a document ending with a code block', () => {
		const fixture = valid.find((candidate) => candidate.name === '06-code-blocks');
		const ed = open(fixture?.value);
		const before = toStored(ed.getJSON()).doc.content ?? [];
		ed.commands.insertContentAt(1, 'X');
		const after = toStored(ed.getJSON()).doc.content ?? [];
		expect(after).toHaveLength(before.length);
		expect(after.at(-1)).toEqual(before.at(-1));
		expect(after[0]?.content?.[0]?.text).toBe(`X${before[0]?.content?.[0]?.text ?? ''}`);
	});
});
