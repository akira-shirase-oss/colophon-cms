<!--
  Choosing uploads for an `asset` field (media M.2, D-095): "Choose image" opens the media library
  panel (MediaLibraryPanel: search, pages, upload inside it, the upload states) over the project's
  uploads. A chosen upload shows as its thumbnail and name with Change and Remove; a list shows
  chips that can be moved up and down and removed, up to its maximum, never the same upload
  twice. The server decides: an id that names no upload is refused, and marked on its chip.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ImageIcon from '@lucide/svelte/icons/image';
	import XIcon from '@lucide/svelte/icons/x';
	import { createQueries } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { getAsset, type Asset, type FieldDefinition } from './api.ts';
	import { stateText, thumbnailUrl } from './media.ts';
	import MediaLibraryPanel from './MediaLibraryPanel.svelte';

	let {
		project,
		field,
		value = $bindable(),
		disabled = false,
		input,
		describedBy,
		refusedAt = null,
		refusal = '',
		onchange = () => {}
	}: {
		project: string;
		field: Extract<FieldDefinition, { type: 'asset' }>;
		/** One id ('' for none), or the list of ids. */
		value: string | string[];
		disabled?: boolean;
		/** The library toggle's id, which the field's label names. */
		input: string;
		describedBy?: string;
		/** For a list: the place of the id the server refused. */
		refusedAt?: number | null;
		refusal?: string;
		onchange?: () => void;
	} = $props();

	const chosen = $derived(Array.isArray(value) ? value : value === '' ? [] : [value]);
	const full = $derived(field.many ? chosen.length >= field.max : chosen.length >= 1);

	// Each chosen upload's record: its name and state. A refused read is an upload the role
	// can't see (or one that isn't there).
	const records = createQueries(() => ({ queries: chosen.map((id) => ({ queryKey: ['asset', project, id], queryFn: () => getAsset(project, id), staleTime: 5_000, retry: false })) }));
	const named = $derived(chosen.map((id, index) => ({ id, asset: records[index]?.data, failed: records[index]?.isError === true })));
	const label = (asset: Asset | undefined, failed: boolean) => (failed ? 'An upload you can’t see' : asset === undefined ? 'Loading…' : asset.file_name);

	// The library panel, shown while it is open.
	let open = $state(false);

	function set(next: string[]) {
		value = field.many ? next : (next[0] ?? '');
		onchange();
	}
	function choose(id: string) {
		if (chosen.includes(id)) return;
		set(field.many ? [...chosen, id] : [id]);
		if (!field.many) open = false;
	}
	function move(index: number, by: number) {
		const next = [...chosen];
		const [item] = next.splice(index, 1);
		if (item !== undefined) next.splice(index + by, 0, item);
		set(next);
	}
</script>

<div class="grid gap-2" data-testid="asset-{field.id}">
	{#if named.length > 0}
		<ol class="grid gap-1.5" aria-label="{field.label}: chosen">
			{#each named as item, index (item.id)}
				<li
					class="flex min-w-0 items-center gap-2 rounded-lg border bg-card px-3 py-1.5 text-sm {refusedAt === index ? 'border-destructive' : 'border-border'}"
					data-testid="asset-chip"
					data-invalid={refusedAt === index || undefined}
				>
					{#if item.asset?.state === 'usable'}
						<img src={thumbnailUrl(project, item.asset)} alt="" class="size-8 shrink-0 rounded object-cover" />
					{:else}
						<ImageIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
					{/if}
					<span class="min-w-0 flex-1 truncate" data-user-content>{label(item.asset, item.failed)}</span>
					{#if item.asset && item.asset.state !== 'usable'}
						<span class="text-xs text-muted-foreground">{stateText(item.asset.state)}</span>
					{/if}
					{#if refusedAt === index}<span class="text-xs text-destructive" title={refusal}>Not found</span>{/if}
					{#if field.many && !disabled}
						<Button variant="ghost" size="icon-xs" aria-label="Move {label(item.asset, item.failed)} up" disabled={index === 0} onclick={() => move(index, -1)}><ArrowUpIcon /></Button>
						<Button variant="ghost" size="icon-xs" aria-label="Move {label(item.asset, item.failed)} down" disabled={index === named.length - 1} onclick={() => move(index, 1)}><ArrowDownIcon /></Button>
					{/if}
					{#if !disabled}
						<Button variant="ghost" size="icon-xs" aria-label="Remove {label(item.asset, item.failed)}" onclick={() => set(chosen.filter((id) => id !== item.id))}><XIcon /></Button>
					{/if}
				</li>
			{/each}
		</ol>
	{/if}

	{#if !disabled}
		{#if full}
			<p class="text-xs text-muted-foreground">
				{#if field.many}At most {field.max} {field.max === 1 ? 'image' : 'images'}: remove one to add another.{:else}Remove it to choose another.{/if}
			</p>
		{:else}
			<div class="grid max-w-xl gap-1.5">
				<Button
					variant="outline"
					class="w-fit"
					id={input}
					aria-describedby={describedBy}
					aria-expanded={open}
					onclick={() => (open = !open)}
					data-testid="asset-toggle"
				>
					{chosen.length === 0 ? 'Choose an image' : field.many ? 'Add an image' : 'Change the image'}
				</Button>
				{#if open}
					<div class="rounded-lg border border-border bg-card p-3">
						<MediaLibraryPanel {project} {chosen} onpick={(asset) => choose(asset.id)} allowDelete label="{field.label}: uploads" />
					</div>
				{/if}
			</div>
		{/if}
	{/if}
</div>
