// An app's image node view (the Studio's real picture, state and buttons) changes how an image
// looks while editing, never what is stored: every valid fixture still opens exactly (D-022), an
// inserted image is still `{assetId, alt}`, and Enter on a selected image asks the app to edit
// its alt text instead of changing the document.

import type { Editor, NodeViewRenderer } from '@tiptap/core';
import { NodeSelection } from '@tiptap/pm/state';
import { afterEach, describe, expect, test } from 'vitest';
import { IMAGE_ACTION, type ImageAction, openEditor, type RichTextValue, serialize, toStored } from '../src/index.ts';
import { valid } from './fixtures.ts';

const ASSET = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d';
const withImage: RichTextValue = {
	schema: 'colophon/rich-text',
	version: 1,
	doc: {
		type: 'doc',
		content: [
			{ type: 'paragraph', content: [{ type: 'text', text: 'Before' }] },
			{ type: 'image', attrs: { assetId: ASSET, alt: 'A diagram' } }
		]
	}
};

/** A stand-in for the Studio's view: a picture the format never sees. */
const drawn: string[] = [];
const view: NodeViewRenderer = ({ node }) => {
	const dom = document.createElement('figure');
	const img = document.createElement('img');
	img.setAttribute('src', `/media/v1/p/production/${String(node.attrs['assetId'])}/w320`);
	dom.append(img);
	drawn.push(String(node.attrs['assetId']));
	return { dom, ignoreMutation: () => true };
};

let editor: Editor | undefined;
afterEach(() => {
	editor?.destroy();
	editor = undefined;
	drawn.length = 0;
});

function enter(ed: Editor): boolean {
	const event = new KeyboardEvent('keydown', { key: 'Enter', bubbles: true, cancelable: true });
	return ed.view.someProp('handleKeyDown', (handle) => handle(ed.view, event)) ?? false;
}

describe('an image node view changes nothing that is stored', () => {
	test.each(valid.map((fixture) => [fixture.name, fixture.value] as const))('%s opens exactly with a view', (_name, value) => {
		editor = openEditor(value, {}, { image: { view } });
		expect(serialize(toStored(editor.getJSON()))).toBe(serialize(value));
	});

	test('the view draws each image, and inserting one stores only its id and alt', () => {
		editor = openEditor(withImage, { element: document.createElement('div') }, { image: { view } });
		expect(drawn).toContain(ASSET);
		const other = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4e';
		editor.commands.setTextSelection(1);
		expect(editor.commands.insertImage({ assetId: other, alt: '  The new picture ' })).toBe(true);
		const images = JSON.stringify(toStored(editor.getJSON()).doc);
		expect(images).toContain(`{"type":"image","attrs":{"assetId":"${other}","alt":"The new picture"}}`);
		expect(images).not.toContain('/media/');
	});
});

describe('Enter on a selected image', () => {
	test('asks the app to edit its alt text, and leaves the document as it was', () => {
		editor = openEditor(withImage, { element: document.createElement('div') }, { image: { view } });
		const ed = editor;
		const asked: ImageAction[] = [];
		ed.view.dom.addEventListener(IMAGE_ACTION, (event) => asked.push((event as CustomEvent<ImageAction>).detail));
		const before = serialize(toStored(ed.getJSON()));
		let at = -1;
		ed.state.doc.descendants((node, pos) => {
			if (node.type.name === 'image') at = pos;
		});
		ed.view.dispatch(ed.state.tr.setSelection(NodeSelection.create(ed.state.doc, at)));
		expect(enter(ed)).toBe(true);
		expect(asked).toEqual([{ action: 'edit', pos: at }]);
		expect(serialize(toStored(ed.getJSON()))).toBe(before);
	});

	test('without a view (no media library) it does what Enter always did', () => {
		editor = openEditor(withImage, { element: document.createElement('div') });
		const ed = editor;
		const asked: ImageAction[] = [];
		ed.view.dom.addEventListener(IMAGE_ACTION, (event) => asked.push((event as CustomEvent<ImageAction>).detail));
		let at = -1;
		ed.state.doc.descendants((node, pos) => {
			if (node.type.name === 'image') at = pos;
		});
		ed.view.dispatch(ed.state.tr.setSelection(NodeSelection.create(ed.state.doc, at)));
		enter(ed);
		expect(asked).toEqual([]);
	});

	test('in a read-only editor it asks nothing', () => {
		editor = openEditor(withImage, { element: document.createElement('div'), editable: false }, { image: { view } });
		const ed = editor;
		const asked: ImageAction[] = [];
		ed.view.dom.addEventListener(IMAGE_ACTION, (event) => asked.push((event as CustomEvent<ImageAction>).detail));
		let at = -1;
		ed.state.doc.descendants((node, pos) => {
			if (node.type.name === 'image') at = pos;
		});
		ed.view.dispatch(ed.state.tr.setSelection(NodeSelection.create(ed.state.doc, at)));
		enter(ed);
		expect(asked).toEqual([]);
	});
});
