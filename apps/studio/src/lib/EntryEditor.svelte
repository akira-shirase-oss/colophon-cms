<!--
  The entry editor (steps 1.3 and 1.5): every field of the entry's content type, autosave with a
  status anyone can read, and, when someone else saved first, a choice instead of an overwrite.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { asset } from '$app/paths';
	import type { Editor } from '@tiptap/core';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { openEditor, type RichTextValue, toStored } from '@colophon/editor';
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import { Button } from '@colophon/ui';
	import { type Content, type ContentType, type FieldDefinition, listRevisions, type Loaded, signInUrl } from './api.ts';
	import { EMPTY_DOCUMENT, type FormValue, formValue, storedValue } from './content.ts';
	import { kept } from './drafts.ts';
	import FieldInputs from './FieldInputs.svelte';
	import { Saver } from './saver.svelte.ts';

	let {
		project,
		collection,
		entry,
		type,
		initial
	}: { project: string; collection: string; entry: string; type: ContentType; initial: Loaded } = $props();

	const queryClient = useQueryClient();
	// Until media uploads arrive (P4), every image previews as the same placeholder.
	const assetUrl = (assetId: string) => `${asset('placeholder.svg')}#${assetId}`;
	// The component is keyed on the entry, so the props it starts with are the ones it keeps.
	// svelte-ignore state_referenced_locally
	const definition = type.definition;
	const plain = definition.fields.filter((field) => field.type !== 'rich_text');
	const rich = definition.fields.filter((field) => field.type === 'rich_text');
	// svelte-ignore state_referenced_locally
	const saver = new Saver(project, collection, entry, definition, initial, (loaded) => {
		queryClient.setQueryData(['draft', project, collection, entry], loaded);
		void queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
		void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	});
	const revisions = createQuery(() => ({
		queryKey: ['history', project, collection, entry],
		queryFn: () => listRevisions(project, collection, entry)
	}));

	const hosts: Record<string, HTMLDivElement> = {};
	let editors: Record<string, Editor> = {};
	let form = $state<Record<string, FormValue>>({});
	let unopenable = $state('');

	function current(): Content {
		const content: Content = {};
		for (const field of definition.fields) {
			const editor = editors[field.id];
			const value = field.type === 'rich_text' ? editor && toStored(editor.getJSON()) : storedValue(field, form[field.id]);
			if (value !== undefined) content[field.id] = value;
		}
		return content;
	}

	function changed() {
		saver.edit(current());
	}

	/** Shows `content`, refusing any rich text the editor can't give back exactly (D-022). */
	function show(content: Content) {
		const next: Record<string, Editor> = {};
		for (const field of rich) {
			try {
				next[field.id] = openEditor((content[field.id] as RichTextValue | undefined) ?? EMPTY_DOCUMENT, {
					editorProps: { attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': field.label } },
					onUpdate: changed
				});
			} catch (error) {
				Object.values(next).forEach((editor) => editor.destroy());
				unopenable = `${field.label}: ${(error as Error).message}`;
				return;
			}
		}
		Object.values(editors).forEach((editor) => editor.destroy());
		for (const field of rich) {
			const host = hosts[field.id];
			const editor = next[field.id];
			if (host && editor) editor.mount(host);
		}
		editors = next;
		form = Object.fromEntries(plain.map((field) => [field.id, formValue(field, content[field.id])]));
		unopenable = '';
	}

	function keepMine() {
		// After a reload the editor shows the server's version; "mine" is what this device kept.
		if (saver.conflict?.restored) show(saver.conflict.mine);
		saver.keepMine();
	}

	function useTheirs() {
		show(saver.useTheirs());
		void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	}

	// Someone else saved: the history this page loaded is out of date.
	$effect(() => {
		if (saver.conflict !== null) void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	});

	function shown(field: FieldDefinition, value: unknown): string {
		if (value === undefined) return '(empty)';
		if (field.type === 'boolean') return value === true ? 'Yes' : 'No';
		if (field.type === 'datetime' && typeof value === 'string') return new Date(value).toLocaleString();
		return String(value);
	}

	const label = $derived.by(() => {
		switch (saver.status) {
			case 'saved':
				return 'Saved';
			case 'unsaved':
				return saver.keptOnDevice ? 'Unsaved changes, kept on this device' : 'Unsaved changes';
			case 'saving':
				return 'Saving…';
			case 'retrying':
				return "Can't reach the server. Your changes are kept on this device; retrying.";
			case 'conflict':
				return 'Not saved: someone else saved first';
			case 'refused':
				return `Not saved: ${saver.message}`;
			case 'signed-out':
				return 'Signed out: changes kept on this device';
		}
	});

	onMount(() => {
		const local = kept(project, entry);
		show(local === null ? initial.draft.content : saver.restore(local, initial.draft));
		const onKey = (event: KeyboardEvent) => {
			if ((event.metaKey || event.ctrlKey) && event.key === 's') {
				event.preventDefault();
				saver.saveNow();
			}
		};
		// Edits are kept on this device, so leaving only warns when they couldn't be.
		const onLeave = (event: BeforeUnloadEvent) => {
			if (saver.status !== 'saved' && !saver.keptOnDevice) event.preventDefault();
		};
		window.addEventListener('keydown', onKey);
		window.addEventListener('beforeunload', onLeave);
		return () => {
			window.removeEventListener('keydown', onKey);
			window.removeEventListener('beforeunload', onLeave);
			saver.destroy();
			Object.values(editors).forEach((editor) => editor.destroy());
		};
	});
</script>

{#snippet version(content: Content)}
	{#each definition.fields as field (field.id)}
		<div class="value">
			<span class="value-label">{field.label}</span>
			{#if field.type === 'rich_text' && content[field.id] !== undefined}
				<RichText value={content[field.id] as RichTextValue} {assetUrl}>
					{#snippet fallback(problem)}<p>This can't be shown: {problem}</p>{/snippet}
				</RichText>
			{:else}
				<span>{shown(field, content[field.id])}</span>
			{/if}
		</div>
	{/each}
{/snippet}

<div class="bar">
	<p class="status {saver.status}" role="status" data-testid="save-status" data-status={saver.status}>{label}</p>
	<Button variant="tonal" onclick={() => saver.saveNow()} disabled={saver.status === 'saved' || saver.status === 'conflict'}>
		Save now
	</Button>
</div>

{#if saver.message && saver.status !== 'refused' && saver.status !== 'conflict'}
	<p class="note" data-testid="save-message">{saver.message}</p>
{/if}
{#if saver.status === 'signed-out'}
	<p class="note"><a href={signInUrl(window.location.pathname)} data-sveltekit-reload>Sign in again</a> to save them.</p>
{/if}
{#if unopenable}
	<p role="alert">This entry can't be opened in the editor without changing it: {unopenable}</p>
{/if}

{#if saver.conflict}
	<section class="conflict" aria-labelledby="conflict-heading" data-testid="conflict">
		<h2 id="conflict-heading">{saver.message}</h2>
		<p>
			<strong>Keep my version</strong> saves yours on top of theirs; theirs stays in the history.
			<strong>Use their version</strong> discards your unsaved changes.
		</p>
		<div class="versions">
			<article data-testid="theirs">
				<h3>Their version</h3>
				{@render version(saver.conflict.theirs.content)}
			</article>
			<article data-testid="mine">
				<h3>Your version</h3>
				{@render version(saver.conflict.mine)}
			</article>
		</div>
		<div class="actions">
			<Button onclick={keepMine}>Keep my version</Button>
			<Button variant="outlined" onclick={useTheirs}>Use their version</Button>
		</div>
	</section>
{/if}

<FieldInputs fields={plain} bind:form titleField={definition.title_field} onchange={changed} />
{#each rich as field (field.id)}
	<div class="rich">
		<p class="rich-label">{field.label}{field.required ? '' : ' (optional)'}</p>
		<div class="editor" bind:this={hosts[field.id]}></div>
	</div>
{/each}

<section class="history">
	<h2>History</h2>
	{#if revisions.data}
		<ol data-testid="history">
			{#each revisions.data as revision (revision.id)}
				<li>{revision.title} <small>{new Date(revision.created_at).toLocaleString()}</small></li>
			{/each}
		</ol>
	{/if}
</section>

<style>
	.bar {
		display: flex;
		gap: 1rem;
		align-items: center;
		justify-content: space-between;
		flex-wrap: wrap;
		margin-bottom: 1rem;
	}
	.status {
		margin: 0;
		font: var(--md-sys-typescale-label-large);
		color: var(--md-sys-color-on-surface-variant);
	}
	.status.saved {
		color: var(--md-sys-color-success, var(--md-sys-color-primary));
	}
	.status.conflict,
	.status.refused,
	.status.signed-out {
		color: var(--md-sys-color-error);
	}
	.note {
		color: var(--md-sys-color-on-surface-variant);
	}
	.rich {
		margin-top: 1rem;
	}
	.rich-label {
		margin: 0 0 0.25rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	.editor :global(.tiptap) {
		min-height: 12rem;
		padding: 1rem 1.25rem;
		border: 1px solid var(--md-sys-color-outline-variant);
		border-radius: var(--md-sys-shape-corner-medium, 12px);
		background: var(--md-sys-color-surface-container-lowest, var(--md-sys-color-surface));
		color: var(--md-sys-color-on-surface);
	}
	.editor :global(.tiptap:focus-visible) {
		outline: 2px solid var(--md-sys-color-primary);
		outline-offset: 1px;
	}
	.conflict {
		margin: 1rem 0;
		padding: 1rem 1.25rem;
		border: 2px solid var(--md-sys-color-error);
		border-radius: var(--md-sys-shape-corner-medium, 12px);
		background: var(--md-sys-color-error-container);
		color: var(--md-sys-color-on-error-container);
	}
	.versions {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(16rem, 1fr));
		gap: 1rem;
		margin: 1rem 0;
	}
	.versions article {
		padding: 0.75rem 1rem;
		border-radius: var(--md-sys-shape-corner-small, 8px);
		background: var(--md-sys-color-surface);
		color: var(--md-sys-color-on-surface);
	}
	.value {
		display: grid;
		gap: 0.125rem;
		margin-bottom: 0.5rem;
	}
	.value-label {
		font: var(--md-sys-typescale-label-medium);
		color: var(--md-sys-color-on-surface-variant);
	}
	.actions {
		display: flex;
		gap: 0.75rem;
		flex-wrap: wrap;
	}
	.history {
		margin-top: 2rem;
	}
</style>
