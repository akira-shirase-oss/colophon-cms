<!--
  Change a collection's fields (D-040): the same builder as New collection, prefilled with the
  definition as read. The save names that version (If-Match) and the server refuses a change
  that would orphan stored entries. Owners and developers only.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import LockIcon from '@lucide/svelte/icons/lock';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { deleteCollection, getContentType } from '#lib/api.ts';
	import { projectQuery } from '#lib/queries.ts';
	import TypeForm from '#lib/TypeForm.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const collection = $derived(page.params.type ?? '');
	const mine = createQuery(() => projectQuery(project));
	const type = createQuery(() => ({ queryKey: ['type', project, collection], queryFn: () => getContentType(project, collection) }));
	const mayChange = $derived(mine.data?.role === 'owner' || mine.data?.role === 'developer');
	let confirming = $state(false);
</script>

<svelte:head><title>Change fields · {type.data?.definition.name ?? collection} · Colophon Studio</title></svelte:head>

{#if type.isPending || mine.isPending}
	<Skeleton class="mb-8 h-10 w-56" />
	<Skeleton class="h-64 w-full max-w-3xl rounded-xl" />
{:else if type.isError}
	<Problem error={type.error} />
{:else if mine.isError}
	<Problem error={mine.error} />
{:else if mine.data && !mayChange}
	<EmptyState icon={LockIcon} title="Only owners and developers change content types">
		You are {mine.data.role === 'editor' ? 'an editor' : 'a publisher'} in this project; ask an owner to change the
		fields.
	</EmptyState>
{:else if type.data}
	<PageHeader title="{type.data.definition.name}: change fields">
		{#snippet description()}
			Add, rename or remove this collection's fields (D-040). A change that would orphan a stored
			entry is refused.
		{/snippet}
	</PageHeader>
	<TypeForm {project} editing={{ collection, definition: type.data.definition, version: type.data.version }} />
	<div class="mt-8 flex justify-end border-t border-border pt-6">
		{#if confirming}
			<Button
				variant="destructive"
				size="sm"
				onclick={async () => {
					try {
						await deleteCollection(project, collection);
						await goto(resolve('/projects/[project]', { project }));
					} catch {
						confirming = false;
						toast.error('Could not delete: the collection still has entries.');
					}
				}}>Really delete?</Button
			>
			<Button variant="outline" size="sm" class="ml-2" onclick={() => (confirming = false)}>Keep it</Button>
		{:else}
			<Button variant="outline" size="sm" onclick={() => (confirming = true)}>
				<Trash2Icon class="size-4" /> Delete collection
			</Button>
		{/if}
	</div>
{/if}
