<script lang="ts">
	import { listenForDraftUpdates } from '@colophon-cms/sdk';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	// Tells the Studio's frame this page is ready — and whether it holds a preview session, so a
	// blocked cookie shows as "open in a new tab" there — then reloads after each saved draft.
	$effect(() => listenForDraftUpdates({ studioOrigin: data.studioOrigin, previewing: data.mode === 'preview' }));
</script>

<svelte:head>
	<title>{data.entry.title}</title>
	{#if data.mode === 'preview'}
		<meta name="robots" content="noindex" />
	{/if}
</svelte:head>

<div class="site" data-studio-origin={data.studioOrigin} data-mode={data.mode}>
	{#if data.session}
		<aside class="banner" data-testid="preview-banner" aria-label="Preview mode">
			<strong>Preview mode</strong>
			<span>
				showing the <span data-testid="preview-source">{data.entry.source}</span> of this
				<span data-testid="preview-scope">{data.session.kind}</span>, ends at
				<time datetime={data.session.endsAt} data-testid="preview-ends">{data.session.endsAtText}</time>
			</span>
			<a href={data.exit} data-sveltekit-reload data-testid="exit-preview">Exit preview</a>
		</aside>
	{:else if data.ended}
		<aside class="banner ended" data-testid="preview-ended">
			<strong>Preview ended</strong>
			<span>({data.ended}) — showing the published page.</span>
			<a href={data.exit} data-sveltekit-reload>Exit preview</a>
		</aside>
	{/if}

	<article>
		<h1 data-testid="title">{data.entry.title}</h1>
		<div data-testid="body">
			{#each data.entry.paragraphs as paragraph, index (index)}
				<p>{paragraph}</p>
			{/each}
		</div>
		{#if data.entry.hidden.length > 0}
			<p class="note">Fields your role cannot read are left out: {data.entry.hidden.join(', ')}.</p>
		{/if}
		<p class="note" data-testid="source">{data.mode === 'preview' ? `draft revision ${data.entry.revision ?? ''}` : 'published'}</p>
	</article>
</div>

<style>
	.site {
		max-width: 44rem;
	}
	.banner {
		display: flex;
		flex-wrap: wrap;
		gap: 0.5rem 1rem;
		align-items: baseline;
		padding: 0.75rem 1rem;
		margin-bottom: 1.5rem;
		border: 1px solid var(--warn);
		border-radius: 6px;
		background: var(--warn-bg);
		color: var(--warn);
	}
	.banner a {
		margin-left: auto;
		color: inherit;
		font-weight: 600;
	}
	.banner.ended {
		border-color: var(--line);
		background: var(--card);
		color: var(--muted);
	}
	.note {
		color: var(--muted);
		font-size: 0.875rem;
	}
</style>
