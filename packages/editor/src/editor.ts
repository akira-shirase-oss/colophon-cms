// Opening a stored document in the editor: lossless, or refused. Never trimmed.
//
// ProseMirror quietly drops what its schema can't hold: unknown nodes and marks (Tiptap's
// content check reports those) but also unknown fields and attributes, which nothing reports.
// So after loading, the document is converted back and compared with what was stored. Any
// difference means saving would change the document, and it is not opened.

import { Editor, type EditorOptions } from '@tiptap/core';
import { extensions } from './extensions.ts';
import { type RichTextValue, serialize, toEditor, toStored, UnrepresentableContent } from './stored.ts';

/** The first JSON Pointer at which two values differ, for the error message. */
function firstDifference(a: unknown, b: unknown, path = ''): string | null {
	if (JSON.stringify(a) === JSON.stringify(b)) return null;
	if (typeof a !== 'object' || typeof b !== 'object' || a === null || b === null) return path || '/';
	const keys = new Set([...Object.keys(a), ...Object.keys(b)]);
	for (const key of keys) {
		const found = firstDifference(
			(a as Record<string, unknown>)[key],
			(b as Record<string, unknown>)[key],
			`${path}/${key.replaceAll('~', '~0').replaceAll('/', '~1')}`
		);
		if (found !== null) return found;
	}
	return path || '/';
}

/** Creates an editor for `value`, or throws `UnrepresentableContent` if opening it would lose
 *  or change anything. */
export function openEditor(value: RichTextValue, options: Partial<EditorOptions> = {}): Editor {
	let problem: Error | undefined;
	const editor = new Editor({
		...options,
		extensions: extensions(),
		content: toEditor(value),
		enableContentCheck: true,
		onContentError: ({ error }) => {
			problem = error;
		}
	});
	if (problem !== undefined) {
		editor.destroy();
		throw new UnrepresentableContent(`the editor can't represent this document: ${problem.message}`);
	}
	let reopened: RichTextValue;
	try {
		reopened = toStored(editor.getJSON());
	} catch (error) {
		editor.destroy();
		throw error;
	}
	if (serialize(reopened) !== serialize(value)) {
		const at = firstDifference(value, reopened);
		editor.destroy();
		throw new UnrepresentableContent(`opening this document would change it at ${at ?? '/'}`);
	}
	return editor;
}
