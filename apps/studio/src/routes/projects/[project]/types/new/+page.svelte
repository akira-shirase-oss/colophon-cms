<!--
  A new collection: its name, the slug the API knows it by, and the fields entries in it carry,
  built field by field (D-040). The title field is the required, single-line text field an entry
  is listed by. Owners and developers only; everyone else is told, not refused mid-form.
-->
<script lang="ts">
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import LockIcon from '@lucide/svelte/icons/lock';
	import { projectQuery } from '#lib/queries.ts';
	import TypeForm from '#lib/TypeForm.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const mine = createQuery(() => projectQuery(project));
	const mayChange = $derived(mine.data?.role === 'owner' || mine.data?.role === 'developer');
</script>

<svelte:head><title>New collection · Colophon Studio</title></svelte:head>

<PageHeader title="New collection">
	{#snippet description()}
		What editors see and the fields entries in it carry, versioned together (D-040). Changing a
		collection later keeps every stored entry valid.
	{/snippet}
</PageHeader>

{#if mine.isError}
	<Problem error={mine.error} />
{:else if mine.data && !mayChange}
	<EmptyState icon={LockIcon} title="Only owners and developers change content types">
		You are {mine.data.role === 'editor' ? 'an editor' : 'a publisher'} in this project; ask an owner to create the
		collection.
	</EmptyState>
{:else}
	<TypeForm {project} />
{/if}
