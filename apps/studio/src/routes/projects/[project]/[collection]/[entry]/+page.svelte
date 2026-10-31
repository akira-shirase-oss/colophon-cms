<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import EntryEditor from '#lib/EntryEditor.svelte';
	import { deleteEntry, getContentType, getEntry } from '#lib/api.ts';
	import { ago, entriesQuery, referencedByQuery } from '#lib/queries.ts';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const collection = $derived(page.params.collection ?? '');
	const entry = $derived(page.params.entry ?? '');
	const type = createQuery(() => ({ queryKey: ['type', project, collection], queryFn: () => getContentType(project, collection) }));
	const links = createQuery(() => referencedByQuery(project, collection, entry));
	let locale = $state('');
	$effect(() => {
		if (locale === '') locale = draft.data?.draft.locale ?? '';
	});
	// Loaded once per visit: after that the editor owns the content, and saving keeps it current.
	const draft = createQuery(() => ({
		queryKey: ['draft', project, collection, entry],
		queryFn: () => getEntry(project, collection, entry),
		staleTime: Infinity,
		refetchOnWindowFocus: false
	}));
	// What readers get now comes with the collection's list (the newest 100).
	const list = createQuery(() => entriesQuery(project, collection));
	const published = $derived(list.data?.data.find((summary) => summary.id === entry)?.published ?? null);
	let confirming = $state(false);
</script>

<svelte:head><title>{draft.data?.draft.revision.title ?? 'Editing'} · Colophon Studio</title></svelte:head>

{#if draft.isPending || type.isPending}
	<Skeleton class="mb-3 h-10 w-2/3" />
	<Skeleton class="mb-8 h-4 w-48" />
	<Skeleton class="h-64 w-full rounded-xl" />
{:else if draft.isError || type.isError}
	<Problem error={(draft.error ?? type.error) as Error} />
{:else}
	<PageHeader title={draft.data.draft.revision.title || 'Untitled'}>
		{#snippet eyebrow()}{type.data.definition.name}{/snippet}
		{#snippet meta()}
			<span>Saved {ago(draft.data.draft.updated_at)}</span>
			<span aria-hidden="true">·</span>
			<span class="font-mono text-xs">{entry.slice(-12)}</span>
		{/snippet}
	</PageHeader>
	<div class="mb-6 flex items-center justify-end gap-2">
		<Input id="entry-locale" class="w-28" placeholder="locale" bind:value={locale} aria-label="Locale" title="The locale this entry is written in — saved with the draft" maxlength={35} />
		{#if confirming}
			<Button
				variant="destructive"
				size="sm"
				onclick={async () => {
					try {
						await deleteEntry(project, collection, entry);
						await goto(resolve('/projects/[project]/[collection]', { project, collection }));
					} catch {
						confirming = false;
						toast.error('Could not delete: the entry is in a changeset or was published.');
					}
				}}>Really delete?</Button
			>
			<Button variant="outline" size="sm" class="ml-2" onclick={() => (confirming = false)}>Keep it</Button>
		{:else}
			<Button variant="outline" size="sm" onclick={() => (confirming = true)}>
				<Trash2Icon class="size-4" /> Delete entry
			</Button>
		{/if}
	</div>
	{#key entry}
		<EntryEditor {project} {collection} {entry} type={type.data} initial={draft.data} {published} locale={() => locale} />
	{/key}
	{#if links.data && links.data.length > 0}
		<section class="mt-8 max-w-3xl" aria-labelledby="referenced-by-heading">
			<h2 id="referenced-by-heading" class="text-base font-semibold">Referenced by</h2>
			<ul class="mt-2 space-y-1" data-testid="referenced-by">
				{#each links.data as link (`${link.collection}/${link.entry}/${link.field}`)}
					<li>
						<a
							class="group flex items-center gap-3 rounded-lg px-2 py-2 hover:bg-muted"
							href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: link.collection, entry: link.entry })}
						>
							<span class="min-w-0 flex-1">
								<span class="block text-sm font-medium">{link.title}</span>
								<span class="text-xs text-muted-foreground">{link.collection_name} · {link.field_label}</span>
							</span>
							<ArrowRightIcon class="size-4 text-muted-foreground opacity-0 transition-opacity group-hover:opacity-100" aria-hidden="true" />
						</a>
					</li>
				{/each}
			</ul>
		</section>
	{/if}
{/if}
