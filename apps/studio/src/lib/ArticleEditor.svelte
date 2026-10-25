<!--
  The article editor (step 1.3): a title, the rich-text body, autosave with a status anyone can
  read, and, when someone else saved first, a choice instead of an overwrite.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { asset } from '$app/paths';
	import type { Editor } from '@tiptap/core';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { openEditor, toStored } from '@colophon/editor';
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import { Button } from '@colophon/ui';
	import { type Article, listRevisions, type Loaded, signInUrl } from './api.ts';
	import { kept } from './drafts.ts';
	import { Saver } from './saver.svelte.ts';

	let { project, entry, initial }: { project: string; entry: string; initial: Loaded } = $props();

	const queryClient = useQueryClient();
	// Until media uploads arrive (P4), every image previews as the same placeholder.
	const assetUrl = (assetId: string) => `${asset('placeholder.svg')}#${assetId}`;
	// The component is keyed on the entry, so the props it starts with are the ones it keeps.
	// svelte-ignore state_referenced_locally
	const saver = new Saver(project, entry, initial, (loaded) => {
		queryClient.setQueryData(['draft', project, entry], loaded);
		void queryClient.invalidateQueries({ queryKey: ['articles', project] });
		void queryClient.invalidateQueries({ queryKey: ['history', project, entry] });
	});
	const revisions = createQuery(() => ({ queryKey: ['history', project, entry], queryFn: () => listRevisions(project, entry) }));

	let host: HTMLDivElement;
	let editor = $state.raw<Editor | null>(null);
	// svelte-ignore state_referenced_locally
	let title = $state(initial.draft.content.title);
	let unopenable = $state('');

	function changed() {
		if (editor) saver.edit({ title, body: toStored(editor.getJSON()) });
	}

	/** Shows `article`, refusing anything the editor can't give back exactly (D-022). */
	function show(article: Article) {
		let created: Editor;
		try {
			created = openEditor(article.body, {
				editorProps: { attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': 'Article body' } },
				onUpdate: changed
			});
		} catch (error) {
			unopenable = (error as Error).message;
			return;
		}
		editor?.destroy();
		created.mount(host);
		editor = created;
		title = article.title;
		unopenable = '';
	}

	function keepMine() {
		// After a reload the editor shows the server's version; "mine" is what this device kept.
		if (saver.conflict?.restored) show(saver.conflict.mine);
		saver.keepMine();
	}

	function useTheirs() {
		show(saver.useTheirs());
		void queryClient.invalidateQueries({ queryKey: ['history', project, entry] });
	}

	// Someone else saved: the history this page loaded is out of date.
	$effect(() => {
		if (saver.conflict !== null) void queryClient.invalidateQueries({ queryKey: ['history', project, entry] });
	});

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
		show(local === null ? initial.draft.content : saver.restore(local.article, local.base, initial.draft));
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
			editor?.destroy();
		};
	});
</script>

<div class="bar">
	<label class="field">
		<span>Title</span>
		<input class="title" bind:value={title} oninput={changed} maxlength="200" />
	</label>
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
	<p role="alert">This article can't be opened in the editor without changing it: {unopenable}</p>
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
				<p class="version-title">{saver.conflict.theirs.content.title}</p>
				<RichText value={saver.conflict.theirs.content.body} {assetUrl} />
			</article>
			<article data-testid="mine">
				<h3>Your version</h3>
				<p class="version-title">{saver.conflict.mine.title}</p>
				<RichText value={saver.conflict.mine.body} {assetUrl}>
					{#snippet fallback(problem)}<p>This version can't be shown: {problem}</p>{/snippet}
				</RichText>
			</article>
		</div>
		<div class="actions">
			<Button onclick={keepMine}>Keep my version</Button>
			<Button variant="outlined" onclick={useTheirs}>Use their version</Button>
		</div>
	</section>
{/if}

<div class="editor" bind:this={host}></div>

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
		align-items: end;
		flex-wrap: wrap;
		margin-bottom: 1rem;
	}
	.field {
		display: grid;
		gap: 0.25rem;
		flex: 1 1 20rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	.title {
		font: var(--md-sys-typescale-headline-small);
		color: var(--md-sys-color-on-surface);
		background: transparent;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-extra-small, 4px);
		padding: 0.5rem 0.75rem;
	}
	.title:focus-visible {
		outline: 2px solid var(--md-sys-color-primary);
		outline-offset: 1px;
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
	.editor :global(.tiptap) {
		min-height: 16rem;
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
	.version-title {
		font: var(--md-sys-typescale-title-medium);
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
