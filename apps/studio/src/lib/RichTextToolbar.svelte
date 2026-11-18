<!--
  Formatting for one rich-text editor: marks, blocks, callouts, links (only the kinds the format
  allows), images from the project's media library with required alt text, undo and redo. One tab
  stop; arrows inside. Headings (H2–H4) and callouts (Note, Tip, Warning) are each one menu, led by
  an icon (D-183), so the toolbar is icons only and wraps less.
  "Insert an image" (gap 3 D1): choose or upload in the library panel, then describe it — alt text
  is required, starts from the upload's own alt text, and is questioned when it reads like a file
  name — then insert the real asset id. The same dialog answers an image's Edit alt text and
  Replace (the `colophon-image` event its node view and Enter send). Without a project (the
  playground) there is no library, so no image button.
-->
<script lang="ts">
	import BoldIcon from '@lucide/svelte/icons/bold';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import CodeIcon from '@lucide/svelte/icons/code';
	import Heading2Icon from '@lucide/svelte/icons/heading-2';
	import Heading3Icon from '@lucide/svelte/icons/heading-3';
	import Heading4Icon from '@lucide/svelte/icons/heading-4';
	import HeadingIcon from '@lucide/svelte/icons/heading';
	import InfoIcon from '@lucide/svelte/icons/info';
	import LightbulbIcon from '@lucide/svelte/icons/lightbulb';
	import MessageSquareQuoteIcon from '@lucide/svelte/icons/message-square-quote';
	import PilcrowIcon from '@lucide/svelte/icons/pilcrow';
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
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import type { Editor } from '@tiptap/core';
	import { createQuery } from '@tanstack/svelte-query';
	import { page } from '$app/state';
	import { type CalloutTone, IMAGE_ACTION, type ImageAction, isAllowedHref, isValidAlt, LIMITS } from '@colophon/editor';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import * as Toolbar from '@colophon/ui/ui/toolbar';
	import { ApiError, getAsset, uploadAsset, type Asset } from './api.ts';
	import { IMAGE_FILE, type ImageFile } from './richTextImage.svelte.ts';
	import { looksLikeFileName, thumbnailUrl } from './media.ts';
	import MediaLibraryPanel from './MediaLibraryPanel.svelte';

	let {
		editor,
		label = 'Formatting',
		project: given = undefined
	}: {
		editor: Editor | null;
		label?: string;
		/** The project whose media library images come from; the page's own when left out. */
		project?: string | null;
	} = $props();
	const project = $derived(given ?? page.params['project'] ?? null);

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

	// The image dialog: what it is doing (insert at the cursor, or edit/replace the image at
	// `pos`), which step it is on, the upload chosen and the alt text being written.
	type ImageFlow = { mode: 'insert'; at?: number } | { mode: 'edit' | 'replace'; pos: number; assetId: string; alt: string };
	let flow = $state<ImageFlow | null>(null);
	let step = $state<'pick' | 'describe'>('pick');
	let picked = $state.raw<Asset | null>(null);
	let imageAlt = $state('');
	let imageProblem = $state('');
	/** A pasted or dropped file on its way into quarantine. */
	let sending = $state('');
	// Describing comes after choosing: the cursor goes to the alt text, not back to the page.
	let altInput = $state<HTMLInputElement | null>(null);
	$effect(() => {
		if (step === 'describe' && flow !== null) altInput?.focus();
	});

	// Editing alt text names the image's upload by id: its record shows beside the text.
	const editing = $derived(flow !== null && flow.mode === 'edit' ? flow.assetId : '');
	const editingRecord = createQuery(() => ({
		queryKey: ['asset', project ?? '', editing],
		queryFn: () => getAsset(project ?? '', editing),
		enabled: project !== null && editing !== '',
		staleTime: 5_000,
		retry: false
	}));
	const shown = $derived(picked ?? (editing === '' ? undefined : editingRecord.data));
	const altLooksLikeName = $derived(looksLikeFileName(imageAlt) || (shown !== undefined && imageAlt.trim() === shown.file_name));

	/** What had focus when the dialog opened (the Image button, the editor, an image's own
	 *  button): a cancel gives focus back to it. */
	let openedFrom: HTMLElement | null = null;

	function openImage(next: ImageFlow) {
		if (flow === null) openedFrom = document.activeElement instanceof HTMLElement ? document.activeElement : null;
		flow = next;
		picked = null;
		imageProblem = '';
		if (next.mode === 'edit') {
			step = 'describe';
			imageAlt = next.alt;
		} else {
			step = 'pick';
			imageAlt = '';
		}
	}

	/** Whether closing should leave the cursor in the editor (an image went in) rather than give
	 *  focus back to what opened the dialog (a cancel). */
	let backToEditor = false;

	function closeImage(placed = false) {
		backToEditor = placed;
		flow = null;
		picked = null;
		imageAlt = '';
		imageProblem = '';
	}

	/** The library answered: describe the chosen upload next, starting from its own alt text (a
	 *  replacement keeps the old words when the new upload has none). */
	function chose(asset: Asset) {
		picked = asset;
		imageAlt = asset.alt?.trim() || (flow !== null && flow.mode === 'replace' ? flow.alt : '');
		imageProblem = '';
		step = 'describe';
	}

	// An image's own buttons and Enter ask through the editor: edit its alt text, or replace it.
	$effect(() => {
		const current = editor;
		if (current === null || project === null) return;
		let dom: HTMLElement;
		try {
			dom = current.view.dom;
		} catch {
			return;
		}
		const asked = (event: Event) => {
			const { action, pos } = (event as CustomEvent<ImageAction>).detail;
			const node = current.state.doc.nodeAt(pos);
			if (node === null || node.type.name !== 'image') return;
			openImage({ mode: action, pos, assetId: String(node.attrs['assetId'] ?? ''), alt: String(node.attrs['alt'] ?? '') });
		};
		const dropped = (event: Event) => void sendFile((event as CustomEvent<ImageFile>).detail);
		dom.addEventListener(IMAGE_ACTION, asked);
		dom.addEventListener(IMAGE_FILE, dropped);
		return () => {
			dom.removeEventListener(IMAGE_ACTION, asked);
			dom.removeEventListener(IMAGE_FILE, dropped);
		};
	});

	/** A pasted or dropped image file: sent into quarantine at once, then described — the image
	 *  goes in where it was dropped, showing Checking… until the worker settles it. */
	async function sendFile({ file, pos }: ImageFile) {
		if (project === null || sending !== '') return;
		openImage({ mode: 'insert', at: pos });
		step = 'describe';
		sending = file.name;
		try {
			const made = await uploadAsset(project, file);
			if (flow === null) return;
			picked = made;
		} catch (error) {
			if (flow === null) return;
			step = 'pick';
			imageProblem = error instanceof ApiError ? `Couldn't upload “${file.name}”: ${error.message}` : `Couldn't upload “${file.name}”.`;
		} finally {
			sending = '';
		}
	}

	function applyImage(event: SubmitEvent) {
		event.preventDefault();
		const current = editor;
		if (current === null || flow === null) return;
		const alt = imageAlt.trim();
		if (!isValidAlt(alt)) {
			imageProblem = 'Describe the image in a few words: alt text is required, up to 300 characters.';
			return;
		}
		let done: boolean;
		if (flow.mode === 'insert') {
			if (picked === null) return;
			const at = flow.at;
			done =
				at === undefined
					? current.chain().focus().insertImage({ assetId: picked.id, alt }).run()
					: current
							.chain()
							.focus()
							.setTextSelection(Math.min(at, current.state.doc.content.size))
							.insertImage({ assetId: picked.id, alt })
							.run();
		} else {
			const pos = flow.pos;
			const assetId = flow.mode === 'replace' && picked !== null ? picked.id : flow.assetId;
			done = current
				.chain()
				.focus()
				.command(({ tr }) => {
					const node = tr.doc.nodeAt(pos);
					if (node === null || node.type.name !== 'image') return false;
					tr.setNodeMarkup(pos, undefined, { ...node.attrs, assetId, alt });
					return true;
				})
				.setNodeSelection(pos)
				.run();
		}
		if (done) closeImage(true);
		else imageProblem = "The image couldn't be placed here. Move the cursor to an empty line and try again.";
	}

	const imageTitle = $derived(
		flow === null ? '' : step === 'pick' ? (flow.mode === 'replace' ? 'Replace the image' : 'Insert an image') : flow.mode === 'edit' ? 'Edit alt text' : 'Describe the image'
	);
	const imageAction = $derived(flow === null ? '' : flow.mode === 'insert' ? 'Insert image' : flow.mode === 'replace' ? 'Replace image' : 'Save alt text');

	const tones: { tone: CalloutTone; name: string; icon: typeof InfoIcon }[] = [
		{ tone: 'note', name: 'Note', icon: InfoIcon },
		{ tone: 'tip', name: 'Tip', icon: LightbulbIcon },
		{ tone: 'warning', name: 'Warning', icon: TriangleAlertIcon }
	];
	const levels = [
		{ level: 2, name: 'Heading 2', icon: Heading2Icon },
		{ level: 3, name: 'Heading 3', icon: Heading3Icon },
		{ level: 4, name: 'Heading 4', icon: Heading4Icon }
	] as const;
	const headingNow = $derived(levels.find(({ level }) => active('heading', { level })));
	const calloutNow = $derived(tones.find(({ tone }) => active('callout', { tone })));
	// A choice in a menu puts the cursor back in the text; Escape gives focus back to the button.
	let fromMenu = false;
	const pick = (command: (editor: Editor) => boolean) => () => {
		fromMenu = true;
		run(command)();
	};
	const afterMenu = (event: Event) => {
		if (fromMenu) {
			event.preventDefault();
			editor?.commands.focus();
		}
		fromMenu = false;
	};
</script>

<Toolbar.Root aria-label={label}>
	<Toolbar.Button pressed={active('bold')} onclick={run((e) => e.chain().focus().toggleBold().run())} title="Bold (⌘B)" aria-label="Bold"><BoldIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('italic')} onclick={run((e) => e.chain().focus().toggleItalic().run())} title="Italic (⌘I)" aria-label="Italic"><ItalicIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('strike')} onclick={run((e) => e.chain().focus().toggleStrike().run())} title="Strikethrough" aria-label="Strikethrough"><StrikethroughIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('code')} onclick={run((e) => e.chain().focus().toggleCode().run())} title="Code" aria-label="Code"><CodeIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('link')} onclick={toggleLink} aria-label="Link"><LinkIcon /></Toolbar.Button>
	<Toolbar.Separator />
	<DropdownMenu.Root>
		<DropdownMenu.Trigger>
			{#snippet child({ props })}
				<Toolbar.Button {...props} pressed={headingNow !== undefined} aria-label="Heading" title="Heading" data-testid="heading-menu">
					{#if headingNow}<headingNow.icon />{:else}<HeadingIcon />{/if}<ChevronDownIcon class="size-3! text-muted-foreground" />
				</Toolbar.Button>
			{/snippet}
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="start" class="w-48" onCloseAutoFocus={afterMenu}>
			<DropdownMenu.Item onSelect={pick((e) => e.chain().focus().setParagraph().run())}><PilcrowIcon class="text-muted-foreground" />Normal text</DropdownMenu.Item>
			{#each levels as { level, name, icon: Icon } (level)}
				<DropdownMenu.Item onSelect={pick((e) => e.chain().focus().toggleHeading({ level }).run())} aria-current={headingNow?.level === level || undefined}>
					<Icon class="text-muted-foreground" />{name}
				</DropdownMenu.Item>
			{/each}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
	<Toolbar.Button pressed={active('bulletList')} onclick={run((e) => e.chain().focus().toggleBulletList().run())} aria-label="Bulleted list"><ListIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('orderedList')} onclick={run((e) => e.chain().focus().toggleOrderedList().run())} aria-label="Numbered list"><ListOrderedIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('blockquote')} onclick={run((e) => e.chain().focus().toggleBlockquote().run())} aria-label="Quote"><QuoteIcon /></Toolbar.Button>
	<Toolbar.Button pressed={active('codeBlock')} onclick={run((e) => e.chain().focus().toggleCodeBlock().run())} aria-label="Code block"><SquareCodeIcon /></Toolbar.Button>
	<Toolbar.Separator />
	<DropdownMenu.Root>
		<DropdownMenu.Trigger>
			{#snippet child({ props })}
				<Toolbar.Button {...props} pressed={calloutNow !== undefined} aria-label="Callout" title="Callout: a note, a tip or a warning" data-testid="callout-menu">
					{#if calloutNow}<calloutNow.icon />{:else}<MessageSquareQuoteIcon />{/if}<ChevronDownIcon class="size-3! text-muted-foreground" />
				</Toolbar.Button>
			{/snippet}
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="start" class="w-48" onCloseAutoFocus={afterMenu}>
			{#each tones as { tone, name, icon: Icon } (tone)}
				<!-- The same tone again takes the callout away; another tone changes it. -->
				<DropdownMenu.Item
					onSelect={pick((e) => (calloutNow?.tone === tone ? e.chain().focus().unsetCallout().run() : active('callout') ? e.chain().focus().updateAttributes('callout', { tone }).run() : e.chain().focus().setCallout(tone).run()))}
					aria-current={calloutNow?.tone === tone || undefined}
				>
					<Icon class="text-muted-foreground" />{name}
				</DropdownMenu.Item>
			{/each}
			{#if calloutNow}
				<DropdownMenu.Separator />
				<DropdownMenu.Item onSelect={pick((e) => e.chain().focus().unsetCallout().run())}>Remove the callout</DropdownMenu.Item>
			{/if}
		</DropdownMenu.Content>
	</DropdownMenu.Root>
	{#if project !== null}
		<Toolbar.Button onclick={() => openImage({ mode: 'insert' })} aria-label="Image" title="Insert an image from the media library"><ImageIcon /></Toolbar.Button>
	{/if}
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

{#if project !== null}
	<Dialog.Root open={flow !== null} onOpenChange={(open) => !open && closeImage()}>
		<Dialog.Content
			class={step === 'pick' ? 'sm:max-w-2xl' : 'sm:max-w-md'}
			data-testid="image-dialog"
			onCloseAutoFocus={(event) => {
				event.preventDefault();
				const from = openedFrom;
				openedFrom = null;
				// The editor itself takes focus through Tiptap, which keeps its selection.
				if (backToEditor || from === null || !from.isConnected || from.classList.contains('ProseMirror')) editor?.commands.focus();
				else from.focus();
				backToEditor = false;
			}}
		>
			<Dialog.Header>
				<Dialog.Title>{imageTitle}</Dialog.Title>
				<Dialog.Description>
					{#if step === 'pick'}
						Choose an upload from the media library, or upload a new image.
					{:else}
						Say what the picture shows, for people who can't see it. Alt text is required.
					{/if}
				</Dialog.Description>
			</Dialog.Header>
			{#if step === 'pick'}
				{#if imageProblem}<p role="alert" class="text-xs text-destructive">{imageProblem}</p>{/if}
				<MediaLibraryPanel project={project} onpick={chose} label="Media library" autofocus />
				<Dialog.Footer>
					<Button type="button" variant="outline" onclick={() => closeImage()}>Cancel</Button>
				</Dialog.Footer>
			{:else}
				<form class="grid gap-4" onsubmit={applyImage}>
					<div class="flex min-w-0 items-center gap-3 rounded-lg border border-border bg-muted/40 p-2" data-testid="image-chosen">
						{#if shown?.state === 'usable'}
							<img src={thumbnailUrl(project, shown)} alt="" class="size-16 shrink-0 rounded-md object-cover" />
						{:else}
							<div class="flex size-16 shrink-0 items-center justify-center rounded-md bg-muted"><ImageIcon class="size-5 text-muted-foreground" aria-hidden="true" /></div>
						{/if}
						<div class="grid min-w-0 flex-1 gap-0.5 text-sm">
							<span class="truncate" data-user-content>{shown?.file_name ?? (sending !== '' ? `Uploading ${sending}…` : editingRecord.isError ? 'An upload this project doesn’t have' : 'Loading…')}</span>
							{#if shown?.state === 'quarantined'}
								<span class="text-xs text-muted-foreground">Still being checked. You can insert it now; publishing waits until it is ready.</span>
							{:else if shown?.state === 'rejected'}
								<span class="text-xs text-destructive">Rejected{shown.rejected_reason ? `: ${shown.rejected_reason}` : ''}. Choose another upload.</span>
							{/if}
						</div>
						{#if flow?.mode !== 'edit'}
							<Button type="button" variant="ghost" size="sm" onclick={() => (step = 'pick')}>Choose another</Button>
						{/if}
					</div>
					<div class="grid gap-1.5">
						<Label for="{id}-alt">Alt text</Label>
						<Input
							id="{id}-alt"
							bind:value={imageAlt}
							maxlength={LIMITS.maxAltLength}
							placeholder="Describe the image for people who can't see it"
							aria-invalid={imageProblem !== '' || undefined}
							aria-describedby="{id}-alt-help{altLooksLikeName ? ` ${id}-alt-warning` : ''}{imageProblem ? ` ${id}-alt-problem` : ''}"
							bind:ref={altInput}
						/>
						<p id="{id}-alt-help" class="text-xs text-muted-foreground">
							For example “Two editors marking up a printed page proof”. Up to {LIMITS.maxAltLength} characters.
						</p>
						{#if altLooksLikeName}
							<p id="{id}-alt-warning" class="flex items-start gap-1.5 text-xs text-attention-ink" data-testid="alt-file-name">
								<TriangleAlertIcon class="mt-px size-3.5 shrink-0" aria-hidden="true" />This looks like a file name. Describe what the image shows instead.
							</p>
						{/if}
						{#if imageProblem}<p id="{id}-alt-problem" role="alert" class="text-xs text-destructive">{imageProblem}</p>{/if}
					</div>
					<Dialog.Footer>
						<Button type="button" variant="outline" onclick={() => closeImage()}>Cancel</Button>
						<Button type="submit" disabled={imageAlt.trim() === '' || (flow?.mode !== 'edit' && shown === undefined) || shown?.state === 'rejected'}>{imageAction}</Button>
					</Dialog.Footer>
				</form>
			{/if}
		</Dialog.Content>
	</Dialog.Root>
{/if}
