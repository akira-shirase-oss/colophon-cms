// The Studio's node view for images inside rich text (gap 3 D1): RichTextImage mounted in the
// editor, fed the node's attributes. It changes how an image looks while editing, never the
// stored JSON (`{assetId, alt}`, D-022): Edit alt text and Replace ask the toolbar's dialog
// through the editor's `colophon-image` event; Remove deletes the node like Backspace would.

import { IMAGE_ACTION, type ImageAction } from '@colophon/editor';
import type { QueryClient } from '@tanstack/svelte-query';
import type { NodeViewRenderer } from '@tiptap/core';
import type { EditorView } from '@tiptap/pm/view';
import { mount, unmount } from 'svelte';
import RichTextImage from './RichTextImage.svelte';

/** Draws each image of an editor in `project` with its picture and state, read through
 *  `queryClient` (so live hints refresh it). */
export function imageView(project: string, queryClient: QueryClient): NodeViewRenderer {
	return ({ node, getPos, editor }) => {
		const dom = document.createElement('div');
		dom.setAttribute('data-colophon-image-view', '');
		dom.contentEditable = 'false';
		const position = (): number | null => {
			const at = getPos();
			return typeof at === 'number' ? at : null;
		};
		const props = $state({
			project,
			queryClient,
			assetId: String(node.attrs['assetId'] ?? ''),
			alt: String(node.attrs['alt'] ?? ''),
			selected: false,
			editable: editor.isEditable,
			act: (action: ImageAction['action']) => {
				const pos = position();
				if (pos === null) return;
				const detail: ImageAction = { action, pos };
				editor.view.dom.dispatchEvent(new CustomEvent(IMAGE_ACTION, { bubbles: true, detail }));
			},
			remove: () => {
				const pos = position();
				const current = pos === null ? null : editor.state.doc.nodeAt(pos);
				if (pos === null || current === null || current.type.name !== 'image') return;
				editor
					.chain()
					.focus()
					.deleteRange({ from: pos, to: pos + current.nodeSize })
					.run();
			}
		});
		const component = mount(RichTextImage, { target: dom, props });
		return {
			dom,
			update: (updated) => {
				if (updated.type.name !== 'image') return false;
				props.assetId = String(updated.attrs['assetId'] ?? '');
				props.alt = String(updated.attrs['alt'] ?? '');
				return true;
			},
			selectNode: () => {
				props.selected = true;
			},
			deselectNode: () => {
				props.selected = false;
			},
			// The buttons are the view's own: ProseMirror must not turn a press into a selection
			// or a drag.
			stopEvent: (event) => event.target instanceof Element && event.target.closest('button, a, input, textarea') !== null,
			ignoreMutation: () => true,
			destroy: () => {
				void unmount(component);
			}
		};
	};
}

/** The event an image file pasted or dropped into the editor sends to its toolbar, which uploads
 *  it through the media pipeline and asks for its alt text before inserting it at `pos`. */
export const IMAGE_FILE = 'colophon-image-file';

/** A pasted or dropped image file, and where it goes. */
export interface ImageFile {
	file: File;
	pos: number;
}

/** The types an upload may claim (module 07); anything else is left to the editor's own paste. */
const UPLOADABLE = ['image/png', 'image/jpeg', 'image/webp'];

function firstImage(files: FileList | null | undefined): File | undefined {
	return [...(files ?? [])].find((file) => UPLOADABLE.includes(file.type));
}

function ask(view: EditorView, file: File, pos: number) {
	const detail: ImageFile = { file, pos };
	view.dom.dispatchEvent(new CustomEvent(IMAGE_FILE, { bubbles: true, detail }));
}

/** `editorProps` that turn a pasted or dropped image file into an upload (gap 3 D1): only files
 *  become images, never a pasted `<img>` address (the editor's paste rules still drop those). */
export const imageFileHandlers = {
	handlePaste: (view: EditorView, event: ClipboardEvent): boolean => {
		const file = view.editable ? firstImage(event.clipboardData?.files) : undefined;
		if (file === undefined) return false;
		ask(view, file, view.state.selection.from);
		return true;
	},
	handleDrop: (view: EditorView, event: DragEvent, _slice: unknown, moved: boolean): boolean => {
		const file = view.editable && !moved ? firstImage(event.dataTransfer?.files) : undefined;
		if (file === undefined) return false;
		event.preventDefault();
		const at = view.posAtCoords({ left: event.clientX, top: event.clientY })?.pos ?? view.state.selection.from;
		ask(view, file, at);
		return true;
	}
};
