<script lang="ts">
	import { onMount } from 'svelte';
	import type { Editor } from '@tiptap/core';
	import {
		type CalloutTone,
		isAllowedHref,
		LIMITS,
		measure,
		openEditor,
		type RichTextValue,
		serialize,
		toStored
	} from '@colophon/editor';
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import { Button, Dialog, Tabs, TextField, Toolbar, ToolbarButton, ToolbarDivider } from '@colophon/ui';
	import { fixtures } from '#lib/fixtures.ts';

	// Until media uploads arrive (P4), every image previews as the same placeholder.
	const assetUrl = (assetId: string) => `/placeholder.svg#${assetId}`;

	const EMPTY: RichTextValue = {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: [{ type: 'paragraph' }] }
	};

	let host: HTMLDivElement;
	let editor = $state.raw<Editor | null>(null);
	let value = $state.raw<RichTextValue>(EMPTY);
	// Bumped on every transaction, so toolbar states recompute.
	let tick = $state(0);
	let selectedFixture = $state('');
	let status = $state<{ kind: 'opened' | 'refused'; message: string } | null>(null);
	let linkOpen = $state(false);
	let linkHref = $state('');
	let linkError = $state('');
	let imageOpen = $state(false);
	let imageAlt = $state('');
	let tab = $state('json');

	let canonical = $derived(serialize(value));
	let size = $derived(measure(value));
	let pretty = $derived(JSON.stringify(value, null, 2));

	/** Opens `next` (refusing anything the editor can't reproduce exactly) and swaps it in. */
	function load(next: RichTextValue, label: string) {
		let created: Editor;
		try {
			created = openEditor(next, {
				// The editing area is a multi-line text box; give it a name screen readers announce.
				editorProps: { attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': 'Article body' } },
				onTransaction: () => {
					value = toStored(created.getJSON());
					tick += 1;
				}
			});
		} catch (error) {
			status = { kind: 'refused', message: `${label}: ${(error as Error).message}` };
			return;
		}
		editor?.destroy();
		created.mount(host);
		editor = created;
		value = toStored(created.getJSON());
		status = { kind: 'opened', message: `${label}: opened` };
	}

	onMount(() => {
		load(EMPTY, 'new document');
		return () => editor?.destroy();
	});

	function loadFixture() {
		const fixture = fixtures.find((candidate) => candidate.id === selectedFixture);
		if (fixture) load(fixture.value, fixture.id);
	}

	function active(name: string, attrs?: Record<string, unknown>): boolean {
		void tick;
		return editor?.isActive(name, attrs) ?? false;
	}

	const run = (command: (editor: Editor) => boolean) => () => {
		if (editor) command(editor);
	};

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

{#snippet jsonPanel()}
	<p class="hint">What Colophon would save: canonical colophon/rich-text v1.</p>
	<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
	<!-- A scrollable region must be reachable by keyboard (WCAG 2.1.1; axe scrollable-region-focusable). -->
	<pre class="json" data-testid="stored" data-json={canonical} tabindex="0" aria-label="Stored JSON">{pretty}</pre>
{/snippet}

{#snippet previewPanel()}
	<div class="preview" data-testid="preview">
		<p class="hint">Rendered by the same renderer the public site uses.</p>
		<RichText {value} {assetUrl}>
			{#snippet fallback(problem)}
				<p class="status refused" data-testid="preview-refused">This document can't be previewed: {problem}. The server would refuse it too.</p>
			{/snippet}
		</RichText>
	</div>
{/snippet}

<section class="spike">
	<div class="editor-card">
		<Toolbar label="Formatting">
			<ToolbarButton pressed={active('bold')} onclick={run((e) => e.chain().focus().toggleBold().run())} title="Bold (⌘B)"><b>B</b></ToolbarButton>
			<ToolbarButton pressed={active('italic')} onclick={run((e) => e.chain().focus().toggleItalic().run())} title="Italic (⌘I)"><i>I</i></ToolbarButton>
			<ToolbarButton pressed={active('strike')} onclick={run((e) => e.chain().focus().toggleStrike().run())} title="Strikethrough"><s>S</s></ToolbarButton>
			<ToolbarButton pressed={active('code')} onclick={run((e) => e.chain().focus().toggleCode().run())} title="Code">&lt;/&gt;</ToolbarButton>
			<ToolbarButton pressed={active('link')} onclick={toggleLink}>Link</ToolbarButton>
			<ToolbarDivider />
			{#each [2, 3, 4] as const as level (level)}
				<ToolbarButton pressed={active('heading', { level })} onclick={run((e) => e.chain().focus().toggleHeading({ level }).run())}>H{level}</ToolbarButton>
			{/each}
			<ToolbarButton pressed={active('bulletList')} onclick={run((e) => e.chain().focus().toggleBulletList().run())}>• List</ToolbarButton>
			<ToolbarButton pressed={active('orderedList')} onclick={run((e) => e.chain().focus().toggleOrderedList().run())}>1. List</ToolbarButton>
			<ToolbarButton pressed={active('blockquote')} onclick={run((e) => e.chain().focus().toggleBlockquote().run())}>Quote</ToolbarButton>
			<ToolbarButton pressed={active('codeBlock')} onclick={run((e) => e.chain().focus().toggleCodeBlock().run())}>Code block</ToolbarButton>
			<ToolbarDivider />
			{#each tones as tone (tone)}
				<ToolbarButton
					pressed={active('callout', { tone })}
					onclick={run((e) => (active('callout') ? e.chain().focus().unsetCallout().run() : e.chain().focus().setCallout(tone).run()))}
				>{tone[0]?.toUpperCase()}{tone.slice(1)}</ToolbarButton>
			{/each}
			<ToolbarButton onclick={() => (imageOpen = true)}>Image</ToolbarButton>
			<ToolbarButton onclick={run((e) => e.chain().focus().setHorizontalRule().run())}>Rule</ToolbarButton>
			<ToolbarDivider />
			<ToolbarButton onclick={run((e) => e.chain().focus().undo().run())} title="Undo (⌘Z)">Undo</ToolbarButton>
			<ToolbarButton onclick={run((e) => e.chain().focus().redo().run())} title="Redo (⌘⇧Z)">Redo</ToolbarButton>
		</Toolbar>

		{#if linkOpen}
			<form class="inline-form" onsubmit={applyLink}>
				<TextField label="Link" bind:value={linkHref} error={linkError} placeholder="https://… or /page" />
				<Button type="submit" variant="tonal">Apply</Button>
			</form>
		{/if}

		<div class="editor" bind:this={host} data-testid="editor"></div>

		<p class="limits" data-testid="limits">
			Depth {size.depth}/{LIMITS.maxDepth} · {size.nodes} nodes/{LIMITS.maxNodes} · {size.bytes.toLocaleString()} bytes/{LIMITS.maxBytes.toLocaleString()}
		</p>
	</div>

	<aside class="side">
		<h2>Load a shared fixture</h2>
		<div class="row">
			<select bind:value={selectedFixture} aria-label="Fixture" data-testid="fixture-select">
				<option value="">Choose…</option>
				{#each ['valid', 'invalid'] as kind (kind)}
					<optgroup label={kind}>
						{#each fixtures.filter((fixture) => fixture.kind === kind) as fixture (fixture.id)}
							<option value={fixture.id}>{fixture.name}</option>
						{/each}
					</optgroup>
				{/each}
			</select>
			<Button variant="outlined" onclick={loadFixture} disabled={selectedFixture === ''}>Open</Button>
		</div>
		{#if status}
			<p class="status {status.kind}" data-testid="open-status" data-kind={status.kind}>{status.message}</p>
		{/if}

		<Tabs
			bind:value={tab}
			label="Output"
			items={[
				{ value: 'json', label: 'Stored JSON', panel: jsonPanel },
				{ value: 'preview', label: 'Preview', panel: previewPanel }
			]}
		/>
	</aside>
</section>

<Dialog bind:open={imageOpen} title="Insert an image" description="Media uploads arrive in a later milestone; for now the image is a placeholder. Alt text is required.">
	<form class="dialog-form" onsubmit={insertImage}>
		<TextField label="Alt text" bind:value={imageAlt} maxlength={LIMITS.maxAltLength} placeholder="Describe the image for people who can't see it" />
		<div class="dialog-actions">
			<Button type="button" variant="text" onclick={() => (imageOpen = false)}>Cancel</Button>
			<Button type="submit" disabled={imageAlt.trim() === ''}>Insert image</Button>
		</div>
	</form>
</Dialog>

<style>
	.spike {
		display: grid;
		grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
		gap: 1.5rem;
		align-items: start;
	}
	@media (max-width: 860px) {
		.spike {
			grid-template-columns: 1fr;
		}
	}
	.editor-card {
		border-radius: var(--md-sys-shape-corner-large);
		overflow: hidden;
		background: var(--md-sys-color-surface-container-low);
		border: 1px solid var(--md-sys-color-outline-variant);
	}
	.inline-form {
		display: flex;
		flex-wrap: wrap;
		gap: 0.75rem;
		align-items: center;
		padding: 0.75rem;
		background: var(--md-sys-color-surface-container);
		border-top: 1px solid var(--md-sys-color-outline-variant);
	}
	.editor {
		background: var(--md-sys-color-surface);
		border-top: 1px solid var(--md-sys-color-outline-variant);
		min-height: 24rem;
	}
	.editor :global(.tiptap) {
		outline: none;
		padding: 1.25rem 1.5rem;
		min-height: 22rem;
	}
	.editor :global(aside[data-callout]),
	.preview :global(.callout) {
		border-left: 4px solid var(--md-sys-color-primary);
		background: var(--md-sys-color-primary-container);
		color: var(--md-sys-color-on-primary-container);
		padding: 0.25rem 1rem;
		margin: 0.75rem 0;
		border-radius: var(--md-sys-shape-corner-extra-small);
	}
	.editor :global(aside[data-tone='tip']),
	.preview :global(.callout-tip) {
		border-left-color: var(--md-sys-color-success);
		background: var(--md-sys-color-success-container);
		color: var(--md-sys-color-on-success-container);
	}
	.editor :global(aside[data-tone='warning']),
	.preview :global(.callout-warning) {
		border-left-color: var(--md-sys-color-tertiary);
		background: var(--md-sys-color-tertiary-container);
		color: var(--md-sys-color-on-tertiary-container);
	}
	.editor :global(aside[data-callout] a),
	.preview :global(.callout a) {
		color: inherit;
	}
	.editor :global(figure[data-colophon-image]) {
		margin: 0.75rem 0;
		padding: 2rem 1rem;
		border: 2px dashed var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-medium);
		text-align: center;
		color: var(--md-sys-color-on-surface-variant);
	}
	.editor :global(figure.ProseMirror-selectednode) {
		border-color: var(--md-sys-color-primary);
	}
	.editor :global(pre),
	.preview :global(pre) {
		background: var(--md-sys-color-surface-container-highest);
		padding: 0.75rem;
		border-radius: var(--md-sys-shape-corner-small);
		overflow-x: auto;
	}
	.editor :global(blockquote),
	.preview :global(blockquote) {
		border-left: 3px solid var(--md-sys-color-outline);
		margin-left: 0;
		padding-left: 1rem;
		color: var(--md-sys-color-on-surface-variant);
	}
	.editor :global(a),
	.preview :global(a) {
		color: var(--md-sys-color-primary);
	}
	.limits,
	.hint {
		margin: 0.5rem 0.75rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	.hint {
		margin: 0 0 0.5rem;
	}
	.side h2 {
		font: var(--md-sys-typescale-title-medium);
		margin: 0 0 0.75rem;
	}
	.row {
		display: flex;
		gap: 0.5rem;
		align-items: center;
	}
	.row select {
		flex: 1;
		min-width: 0;
		height: 2.5rem;
		padding: 0 0.75rem;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-extra-small);
		background: var(--md-sys-color-surface);
		color: var(--md-sys-color-on-surface);
		font: var(--md-sys-typescale-body-large);
	}
	.status {
		font: var(--md-sys-typescale-body-medium);
		padding: 0.5rem 0.75rem;
		border-radius: var(--md-sys-shape-corner-small);
		overflow-wrap: anywhere;
	}
	.status.opened {
		color: var(--md-sys-color-on-success-container);
		background: var(--md-sys-color-success-container);
	}
	.status.refused {
		color: var(--md-sys-color-on-error-container);
		background: var(--md-sys-color-error-container);
	}
	.json {
		margin: 0;
		background: var(--md-sys-color-surface-container);
		border-radius: var(--md-sys-shape-corner-medium);
		padding: 0.75rem;
		max-height: 34rem;
		overflow: auto;
		font-size: 0.75rem;
	}
	.preview {
		background: var(--md-sys-color-surface-container-lowest);
		border-radius: var(--md-sys-shape-corner-medium);
		padding: 0.75rem 1rem 1rem;
	}
	.preview :global(figure) {
		margin: 0.75rem 0;
	}
	.preview :global(img) {
		max-width: 100%;
		height: auto;
	}
	.dialog-form {
		display: grid;
		gap: 1.5rem;
	}
	.dialog-actions {
		display: flex;
		justify-content: flex-end;
		gap: 0.5rem;
	}
</style>
