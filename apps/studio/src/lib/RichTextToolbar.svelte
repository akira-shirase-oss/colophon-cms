<!--
  Formatting for one rich-text editor: marks, blocks, callouts, links (only the kinds the format
  allows), placeholder images with required alt text, undo and redo. One tab stop; arrows inside.
-->
<script lang="ts">
	import BoldIcon from '@lucide/svelte/icons/bold';
	import CodeIcon from '@lucide/svelte/icons/code';
	import ImageIcon from '@lucide/svelte/icons/image';
	import ItalicIcon from '@lucide/svelte/icons/italic';
	import LinkIcon from '@lucide/svelte/icons/link';
	import ListIcon from '@lucide/svelte/icons/list';
	import ListOrderedIcon from '@lucide/svelte/icons/list-ordered';
	import MinusIcon from '@lucide/svelte/icons/minus';
	import QuoteIcon from '@lucide/svelte/icons/quote';
	import Redo2Icon from '@lucide/svelte/icons/redo-2';
	import SquareCodeIcon from '@lucide/svelte/icons/square-code';
	import StrikethroughIcon from '@lucide/svelte/icons/strikethrough';
	import Undo2Icon from '@lucide/svelte/icons/undo-2';
	import type { Editor } from '@tiptap/core';
	import { type CalloutTone, isAllowedHref, LIMITS } from '@colophon/editor';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import * as Toolbar from '@colophon/ui/ui/toolbar';

	let { editor, label = 'Formatting' }: { editor: Editor | null; label?: string } = $props();

	// Bumped on every transaction, so pressed states recompute.
	let tick = $state(0);
	$effect(() => {
		const current = editor;
		if (current === null) return;
		const bump = () => (tick += 1);
		current.on('transaction', bump);
		return () => {
			current.off('transaction', bump);
		};
	});

	function active(name: string, attrs?: Record<string, unknown>): boolean {
		void tick;
		return editor?.isActive(name, attrs) ?? false;
	}
	const run = (command: (editor: Editor) => boolean) => () => {
		if (editor) command(editor);
	};

	const id = $props.id();
	let linkOpen = $state(false);
	let linkHref = $state('');
	let linkError = $state('');
	let imageOpen = $state(false);
	let imageAlt = $state('');

	function toggleLink() {
		linkOpen = !linkOpen;
		linkHref = String(editor?.getAttributes('link')['href'] ?? '');
		linkError = '';
	}

	function applyLink(event: SubmitEvent) {
		event.preventDefault();
		if (!editor) return;
		const href = linkHref.trim();
		if (href === '') {
			editor.chain().focus().extendMarkRange('link').unsetLink().run();
		} else if (!isAllowedHref(href)) {
			linkError = 'Only https:, http: and mailto: links, paths starting with /, and #fragments are allowed.';
			return;
		} else {
			editor.chain().focus().extendMarkRange('link').setLink({ href }).run();
		}
		linkOpen = false;
		linkHref = '';
		linkError = '';
	}

	function insertImage(event: SubmitEvent) {
		event.preventDefault();
		// Media uploads arrive in milestone P4; until then an image is a placeholder with a fresh id.
		if (editor?.chain().focus().insertImage({ assetId: crypto.randomUUID(), alt: imageAlt }).run()) {
			imageOpen = false;
			imageAlt = '';
		}
	}

	const tones: CalloutTone[] = ['note', 'tip', 'warning'];
</script>

<Toolbar.Root aria-label={label}>
	<Toolbar.Button pressed={active('bold')} onclick={run((e) => e.chain().focus().toggleBold().run())} title="Bold (⌘B)" aria-label="Bold"><BoldIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('italic')} onclick={run((e) => e.chain().focus().toggleItalic().run())} title="Italic (⌘I)" aria-label="Italic"><ItalicIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('strike')} onclick={run((e) => e.chain().focus().toggleStrike().run())} title="Strikethrough" aria-label="Strikethrough"><StrikethroughIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('code')} onclick={run((e) => e.chain().focus().toggleCode().run())} title="Code" aria-label="Code"><CodeIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('link')} onclick={toggleLink} aria-label="Link"><LinkIcon /></Toolbar.Button>
	<Toolbar.Separator />
	{#each [2, 3, 4] as const as level (level)}
		<Toolbar.Button pressed={active('heading', { level })} onclick={run((e) => e.chain().focus().toggleHeading({ level }).run())} class="font-serif">H{level}</Toolbar.Button>
	{/each}
	<Toolbar.Button pressed={active('bulletList')} onclick={run((e) => e.chain().focus().toggleBulletList().run())} aria-label="Bulleted list"><ListIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('orderedList')} onclick={run((e) => e.chain().focus().toggleOrderedList().run())} aria-label="Numbered list"><ListOrderedIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('blockquote')} onclick={run((e) => e.chain().focus().toggleBlockquote().run())} aria-label="Quote"><QuoteIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('codeBlock')} onclick={run((e) => e.chain().focus().toggleCodeBlock().run())} aria-label="Code block"><SquareCodeIcon /></Toolbar.Button>
	<Toolbar.Separator />
	{#each tones as tone (tone)}
		<Toolbar.Button
			pressed={active('callout', { tone })}
			onclick={run((e) => (active('callout') ? e.chain().focus().unsetCallout().run() : e.chain().focus().setCallout(tone).run()))}
			>{tone[0]?.toUpperCase()}{tone.slice(1)}</Toolbar.Button
		>
	{/each}
	<Toolbar.Button onclick={() => (imageOpen = true)} aria-label="Image"><ImageIcon /></Toolbar.Button>
	<Toolbar.Button onclick={run((e) => e.chain().focus().setHorizontalRule().run())} aria-label="Rule"><MinusIcon /></Toolbar.Button>
	<Toolbar.Separator />
	<Toolbar.Button onclick={run((e) => e.chain().focus().undo().run())} title="Undo (⌘Z)" aria-label="Undo"><Undo2Icon /></Toolbar.Button>
	<Toolbar.Button onclick={run((e) => e.chain().focus().redo().run())} title="Redo (⌘⇧Z)" aria-label="Redo"><Redo2Icon /></Toolbar.Button>
</Toolbar.Root>

{#if linkOpen}
	<form class="flex flex-wrap items-start gap-2 border-b border-border bg-muted/40 px-3 py-2" onsubmit={applyLink}>
		<div class="grid min-w-64 flex-1 gap-1">
			<Label for="{id}-link" class="sr-only">Link address</Label>
			<Input id="{id}-link" bind:value={linkHref} placeholder="https://… or /page" aria-invalid={linkError !== ''} aria-describedby="{id}-link-error" />
			{#if linkError}<p id="{id}-link-error" role="alert" class="text-xs text-destructive">{linkError}</p>{/if}
		</div>
		<Button type="submit" variant="secondary">Apply</Button>
	</form>
{/if}

<Dialog.Root bind:open={imageOpen}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Insert an image</Dialog.Title>
			<Dialog.Description>Media uploads arrive in a later milestone; for now the image is a placeholder. Alt text is required.</Dialog.Description>
		</Dialog.Header>
		<form class="grid gap-4" onsubmit={insertImage}>
			<div class="grid gap-1.5">
				<Label for="{id}-alt">Alt text</Label>
				<Input id="{id}-alt" bind:value={imageAlt} maxlength={LIMITS.maxAltLength} placeholder="Describe the image for people who can't see it" />
			</div>
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (imageOpen = false)}>Cancel</Button>
				<Button type="submit" disabled={imageAlt.trim() === ''}>Insert image</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
