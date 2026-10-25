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
	let tab = $state<'json' | 'preview'>('json');

	let canonical = $derived(serialize(value));
	let size = $derived(measure(value));
	let pretty = $derived(JSON.stringify(value, null, 2));

	/** Opens `next` (refusing anything the editor can't reproduce exactly) and swaps it in. */
	function load(next: RichTextValue, label: string) {
		let created: Editor;
		try {
			created = openEditor(next, {
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

<section class="spike">
	<div class="editor-column">
		<div class="toolbar" role="toolbar" aria-label="Formatting">
			<button aria-pressed={active('bold')} onclick={run((e) => e.chain().focus().toggleBold().run())} title="Bold (⌘B)"><b>B</b></button>
			<button aria-pressed={active('italic')} onclick={run((e) => e.chain().focus().toggleItalic().run())} title="Italic (⌘I)"><i>I</i></button>
			<button aria-pressed={active('strike')} onclick={run((e) => e.chain().focus().toggleStrike().run())} title="Strikethrough"><s>S</s></button>
			<button aria-pressed={active('code')} onclick={run((e) => e.chain().focus().toggleCode().run())} title="Code">&lt;/&gt;</button>
			<button aria-pressed={active('link')} onclick={() => { linkOpen = !linkOpen; linkHref = String(editor?.getAttributes('link')['href'] ?? ''); linkError = ''; }}>Link</button>
			<span class="sep"></span>
			{#each [2, 3, 4] as const as level (level)}
				<button aria-pressed={active('heading', { level })} onclick={run((e) => e.chain().focus().toggleHeading({ level }).run())}>H{level}</button>
			{/each}
			<button aria-pressed={active('bulletList')} onclick={run((e) => e.chain().focus().toggleBulletList().run())}>• List</button>
			<button aria-pressed={active('orderedList')} onclick={run((e) => e.chain().focus().toggleOrderedList().run())}>1. List</button>
			<button aria-pressed={active('blockquote')} onclick={run((e) => e.chain().focus().toggleBlockquote().run())}>Quote</button>
			<button aria-pressed={active('codeBlock')} onclick={run((e) => e.chain().focus().toggleCodeBlock().run())}>Code block</button>
			<span class="sep"></span>
			{#each tones as tone (tone)}
				<button aria-pressed={active('callout', { tone })} onclick={run((e) => (active('callout') ? e.chain().focus().unsetCallout().run() : e.chain().focus().setCallout(tone).run()))}>{tone[0]?.toUpperCase()}{tone.slice(1)}</button>
			{/each}
			<button onclick={() => { imageOpen = !imageOpen; }}>Image</button>
			<button onclick={run((e) => e.chain().focus().setHorizontalRule().run())}>Rule</button>
			<span class="sep"></span>
			<button onclick={run((e) => e.chain().focus().undo().run())} title="Undo (⌘Z)">Undo</button>
			<button onclick={run((e) => e.chain().focus().redo().run())} title="Redo (⌘⇧Z)">Redo</button>
		</div>

		{#if linkOpen}
			<form class="inline-form" onsubmit={applyLink}>
				<label>Link <input bind:value={linkHref} placeholder="https://… or /page" aria-describedby="link-error" /></label>
				<button type="submit">Apply</button>
				{#if linkError}<p id="link-error" class="error" role="alert">{linkError}</p>{/if}
			</form>
		{/if}
		{#if imageOpen}
			<form class="inline-form" onsubmit={insertImage}>
				<label>Alt text <input bind:value={imageAlt} maxlength={LIMITS.maxAltLength} placeholder="Describe the image for people who can't see it" /></label>
				<button type="submit" disabled={imageAlt.trim() === ''}>Insert image</button>
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
			<button onclick={loadFixture} disabled={selectedFixture === ''}>Open</button>
		</div>
		{#if status}
			<p class="status {status.kind}" data-testid="open-status" data-kind={status.kind}>{status.message}</p>
		{/if}

		<div class="tabs" role="tablist" aria-label="Output">
			<button role="tab" aria-selected={tab === 'json'} onclick={() => (tab = 'json')}>Stored JSON</button>
			<button role="tab" aria-selected={tab === 'preview'} onclick={() => (tab = 'preview')}>Preview</button>
		</div>
		<div hidden={tab !== 'json'}>
			<p class="hint">What Colophon would save: canonical colophon/rich-text v1.</p>
			<pre data-testid="stored" data-json={canonical}>{pretty}</pre>
		</div>
		<div hidden={tab !== 'preview'} class="preview" data-testid="preview">
			<p class="hint">Rendered by the same renderer the public site uses.</p>
			<RichText {value} {assetUrl}>
				{#snippet fallback(problem)}
					<p class="status refused" data-testid="preview-refused">This document can't be previewed: {problem}. The server would refuse it too.</p>
				{/snippet}
			</RichText>
		</div>
	</aside>
</section>

<style>
	.spike {
		display: grid;
		grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
		gap: 1.25rem;
	}
	@media (max-width: 860px) {
		.spike {
			grid-template-columns: 1fr;
		}
	}
	.toolbar {
		display: flex;
		flex-wrap: wrap;
		gap: 0.25rem;
		padding: 0.4rem;
		border: 1px solid var(--line);
		border-radius: 10px 10px 0 0;
		background: var(--card);
	}
	.toolbar button,
	.inline-form button,
	.row button {
		font: inherit;
		font-size: 0.85rem;
		padding: 0.3rem 0.55rem;
		border: 1px solid transparent;
		border-radius: 6px;
		background: transparent;
		color: var(--fg);
		cursor: pointer;
	}
	.toolbar button:hover {
		background: var(--line);
	}
	.toolbar button[aria-pressed='true'] {
		background: var(--accent);
		color: var(--bg);
	}
	.inline-form button,
	.row button {
		border-color: var(--line);
	}
	button:disabled {
		opacity: 0.5;
		cursor: not-allowed;
	}
	.sep {
		width: 1px;
		background: var(--line);
		margin: 0 0.25rem;
	}
	.inline-form {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem;
		align-items: center;
		padding: 0.5rem;
		border: 1px solid var(--line);
		border-top: 0;
		background: var(--card);
	}
	.inline-form input {
		font: inherit;
		min-width: 18rem;
		margin-left: 0.4rem;
		padding: 0.3rem 0.5rem;
	}
	.error {
		color: var(--bad);
		margin: 0;
		width: 100%;
	}
	.editor {
		border: 1px solid var(--line);
		border-top: 0;
		border-radius: 0 0 10px 10px;
		background: var(--card);
		min-height: 22rem;
	}
	.editor :global(.tiptap) {
		outline: none;
		padding: 1rem 1.25rem;
		min-height: 20rem;
		line-height: 1.6;
	}
	.editor :global(aside[data-callout]) {
		border-left: 4px solid var(--accent);
		background: var(--bg);
		padding: 0.25rem 0.9rem;
		margin: 0.75rem 0;
		border-radius: 4px;
	}
	.editor :global(aside[data-tone='tip']) {
		border-left-color: var(--ok);
	}
	.editor :global(aside[data-tone='warning']) {
		border-left-color: var(--warn);
	}
	.editor :global(figure[data-colophon-image]) {
		margin: 0.75rem 0;
		padding: 2rem 1rem;
		border: 2px dashed var(--line);
		border-radius: 8px;
		text-align: center;
		color: var(--muted);
	}
	.editor :global(figure.ProseMirror-selectednode) {
		border-color: var(--accent);
	}
	.editor :global(pre) {
		background: var(--bg);
		padding: 0.75rem;
		border-radius: 6px;
	}
	.editor :global(blockquote) {
		border-left: 3px solid var(--line);
		margin-left: 0;
		padding-left: 1rem;
		color: var(--muted);
	}
	.limits,
	.hint {
		color: var(--muted);
		font-size: 0.85rem;
	}
	.tabs {
		display: flex;
		gap: 0.25rem;
		margin: 1rem 0 0.25rem;
	}
	.tabs button {
		font: inherit;
		font-size: 0.9rem;
		padding: 0.35rem 0.75rem;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: transparent;
		color: var(--fg);
		cursor: pointer;
	}
	.tabs button[aria-selected='true'] {
		background: var(--fg);
		color: var(--bg);
	}
	.preview {
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--card);
		padding: 0 1rem 1rem;
		line-height: 1.6;
	}
	.preview :global(.callout) {
		border-left: 4px solid var(--accent);
		background: var(--bg);
		padding: 0.25rem 0.9rem;
		margin: 0.75rem 0;
	}
	.preview :global(.callout-tip) {
		border-left-color: var(--ok);
	}
	.preview :global(.callout-warning) {
		border-left-color: var(--warn);
	}
	.preview :global(figure) {
		margin: 0.75rem 0;
	}
	.preview :global(img) {
		max-width: 100%;
		height: auto;
	}
	.preview :global(pre) {
		background: var(--bg);
		padding: 0.75rem;
		border-radius: 6px;
		overflow-x: auto;
	}
	.side h2 {
		font-size: 1rem;
		margin: 0.25rem 0 0.5rem;
	}
	.row {
		display: flex;
		gap: 0.5rem;
	}
	.row select {
		font: inherit;
		flex: 1;
		min-width: 0;
	}
	.status {
		font-size: 0.85rem;
		padding: 0.4rem 0.6rem;
		border-radius: 6px;
		overflow-wrap: anywhere;
	}
	.status.opened {
		color: var(--ok);
		background: var(--ok-bg);
	}
	.status.refused {
		color: var(--bad);
		background: var(--bad-bg);
	}
	pre {
		background: var(--card);
		border: 1px solid var(--line);
		border-radius: 8px;
		padding: 0.75rem;
		max-height: 34rem;
		overflow: auto;
		font-size: 0.75rem;
	}
</style>
