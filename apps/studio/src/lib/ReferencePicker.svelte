<!--
  A reference field's picker: a select of the entries in the collection it points into, each
  named by its title. A stored id that isn't in the newest hundred stays as the select's value
  so nothing is silently dropped.
-->
<script lang="ts">
	import { createQuery } from '@tanstack/svelte-query';
	import { entriesQuery } from './queries.ts';

	let {
		project,
		to,
		value = $bindable(''),
		required = false,
		onchange = () => {}
	}: {
		project: string;
		to: string;
		value?: string;
		required?: boolean;
		onchange?: () => void;
	} = $props();

	const entries = createQuery(() => entriesQuery(project, to));
</script>

<select class="select max-w-72" bind:value {onchange} aria-busy={entries.isPending}>
	{#if !required}<option value="">None</option>{/if}
	{#each entries.data?.data ?? [] as entry (entry.id)}
		<option value={entry.id}>{entry.revision.title || 'Untitled'}</option>
	{/each}
	{#if value !== '' && !(entries.data?.data ?? []).some((entry) => entry.id === value)}
		<option value={value}>{value.slice(-12)} (not in the newest hundred)</option>
	{/if}
</select>
{#if entries.isError}<p class="text-sm text-muted-foreground">Could not load {to} entries.</p>{/if}
