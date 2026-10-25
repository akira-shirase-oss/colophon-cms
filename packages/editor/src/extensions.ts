// Tiptap configured to exactly the colophon/rich-text v1 node and mark set: StarterKit minus
// what the format doesn't allow, plus the two Colophon nodes (callout and image).

import { Extension, type Extensions, mergeAttributes, Node } from '@tiptap/core';
import StarterKit from '@tiptap/starter-kit';
import { ASSET_ID, type CalloutTone, HEADING_LEVELS, isAllowedHref, isCalloutTone, isValidAlt, LIMITS } from './schema.ts';
import { Plugin, PluginKey } from '@tiptap/pm/state';
import type { Node as ProseMirrorNode } from '@tiptap/pm/model';

declare module '@tiptap/core' {
	interface Commands<ReturnType> {
		callout: {
			/** Wraps the selected paragraphs in a callout. */
			setCallout: (tone: CalloutTone) => ReturnType;
			/** Lifts the selection out of its callout. */
			unsetCallout: () => ReturnType;
		};
		image: {
			/** Inserts an image that refers to an asset in the media library. */
			insertImage: (attrs: { assetId: string; alt: string }) => ReturnType;
		};
	}
}

/** A highlighted block of paragraphs: a note, a tip or a warning. */
export const Callout = Node.create({
	name: 'callout',
	group: 'block',
	content: 'paragraph+',
	defining: true,
	addAttributes() {
		return {
			tone: {
				default: 'note',
				parseHTML: (element) => {
					const tone = element.getAttribute('data-tone');
					return isCalloutTone(tone) ? tone : 'note';
				},
				renderHTML: (attributes) => ({ 'data-tone': attributes['tone'] as string })
			}
		};
	},
	parseHTML() {
		return [{ tag: 'aside[data-callout]' }];
	},
	renderHTML({ HTMLAttributes }) {
		return ['aside', mergeAttributes({ 'data-callout': '' }, HTMLAttributes), 0];
	},
	addCommands() {
		return {
			setCallout:
				(tone) =>
				({ commands }) =>
					commands.wrapIn(this.name, { tone }),
			unsetCallout:
				() =>
				({ commands }) =>
					commands.lift(this.name)
		};
	}
});

/**
 * An image from Colophon's media library: an asset id and its alt text, never a URL. Pasted
 * `<img>` tags from other sites are not turned into images; uploads go through the media
 * pipeline (module 07).
 */
export const Image = Node.create({
	name: 'image',
	group: 'block',
	atom: true,
	draggable: true,
	selectable: true,
	addAttributes() {
		return {
			assetId: {
				default: null,
				parseHTML: (element) => element.getAttribute('data-asset-id'),
				renderHTML: (attributes) => ({ 'data-asset-id': attributes['assetId'] as string })
			},
			alt: {
				default: '',
				parseHTML: (element) => element.getAttribute('data-alt') ?? '',
				renderHTML: (attributes) => ({ 'data-alt': attributes['alt'] as string })
			}
		};
	},
	parseHTML() {
		return [{ tag: 'figure[data-colophon-image]' }];
	},
	renderHTML({ node, HTMLAttributes }) {
		const alt = String(node.attrs['alt'] ?? '');
		return [
			'figure',
			mergeAttributes({ 'data-colophon-image': '' }, HTMLAttributes),
			['figcaption', {}, alt === '' ? 'Image without alt text' : `Image: ${alt}`]
		];
	},
	addCommands() {
		return {
			// Refuses an asset id that isn't a UUID and alt text that is blank or too long, so the
			// editor can't create an image the format rejects.
			insertImage:
				(attrs) =>
				({ commands }) => {
					const alt = attrs.alt.trim();
					if (!ASSET_ID.test(attrs.assetId) || !isValidAlt(alt)) return false;
					return commands.insertContent({ type: this.name, attrs: { assetId: attrs.assetId, alt } });
				}
		};
	}
});

/** Depth as the format counts it: the document is 1, each level of content adds 1. */
function depthOf(node: ProseMirrorNode): number {
	let deepest = 1;
	const walk = (current: ProseMirrorNode, depth: number) => {
		deepest = Math.max(deepest, depth);
		current.forEach((child) => walk(child, depth + 1));
	};
	walk(node, 1);
	return deepest;
}

/**
 * Refuses any change that would nest the document deeper than the format allows (16), or give
 * it more than 10,000 nodes, so the editor can't produce a document the server will reject.
 * Size in bytes is shown to the writer and enforced by the server.
 */
export const Limits = Extension.create({
	name: 'colophonLimits',
	addProseMirrorPlugins() {
		return [
			new Plugin({
				key: new PluginKey('colophonLimits'),
				filterTransaction: (transaction) => {
					if (!transaction.docChanged) return true;
					let nodes = 1;
					transaction.doc.descendants(() => {
						nodes += 1;
					});
					return nodes <= LIMITS.maxNodes && depthOf(transaction.doc) <= LIMITS.maxDepth;
				}
			})
		];
	}
});

/** The extension set for colophon/rich-text v1. */
export function extensions(): Extensions {
	return [
		StarterKit.configure({
			heading: { levels: [...HEADING_LEVELS] },
			// Not in the format.
			underline: false,
			// After any edit it appends an invisible empty paragraph to a document that ends in a
			// code block, image or callout, so changing one word would change the end too. An edit
			// must change only what was edited (test/editing.test.ts).
			trailingNode: false,
			link: {
				openOnClick: false,
				autolink: true,
				linkOnPaste: true,
				defaultProtocol: 'https',
				isAllowedUri: (url) => isAllowedHref(url),
				HTMLAttributes: { rel: 'noopener noreferrer nofollow', target: null }
			}
		}),
		Callout,
		Image,
		Limits
	];
}
