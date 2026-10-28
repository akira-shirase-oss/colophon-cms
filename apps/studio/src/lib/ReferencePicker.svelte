<!--
  Choosing linked entries (step M.2, D-065): a search over the target collection's titles (the
  API's `q`), and the chosen entries by title. One link shows one chosen entry with Change and
  Remove; a list shows chips that can be moved up and down and removed, up to its maximum, never
  the same entry twice. A chosen entry the role can't read shows as "An entry you can't see", never
  as its id. The server decides: an id that names no entry is refused, and marked on its chip.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import LinkIcon from '@lucide/svelte/icons/link';
	import XIcon from '@lucide/svelte/icons/x';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, entryState, type FieldDefinition, searchEntries } from './api.ts';
	import { typesQuery } from './queries.ts';
	import { linkedQuery } from './references.ts';
	import StatusBadge from './shell/StatusBadge.svelte';
	import { shownTitle } from './words.ts';

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
		field: Extract<FieldDefinition, { type: 'reference' }>;
		/** One id ('' for none), or the list of ids. */
		value: string | string[];
		disabled?: boolean;
		/** The search box's id, which the field's label names. */
		input: string;
		describedBy?: string;
		/** For a list: the place of the id the server refused. */
		refusedAt?: number | null;
		refusal?: string;
		onchange?: () => void;
	} = $props();

	const chosen = $derived(Array.isArray(value) ? value : value === '' ? [] : [value]);
	const full = $derived(field.many ? chosen.length >= field.max : chosen.length >= 1);
	const types = createQuery(() => typesQuery(project));
	const target = $derived(types.data?.find((type) => type.slug === field.to)?.definition.name ?? field.to);

	// Each chosen entry's title, read on its own: a refused read is an entry the role can't see.
	const titles = createQueries(() => ({ queries: chosen.map((id) => linkedQuery(project, field.to, id)) }));
	const named = $derived(chosen.map((id, index) => ({ id, read: titles[index]?.data, failed: titles[index]?.isError === true })));
	const label = (read: { title: string; seen: boolean } | undefined, failed: boolean) =>
		failed ? 'Couldn’t load its title' : read === undefined ? 'Loading…' : read.seen ? shownTitle(read.title) : 'An entry you can’t see';

	// Search: the target collection's titles as you type, after a short pause.
	let searching = $state(false);
	let typed = $state('');
	let q = $state('');
	$effect(() => {
		const next = typed.trim();
		const timer = setTimeout(() => (q = next), 200);
		return () => clearTimeout(timer);
	});
	const found = createQuery(() => ({
		queryKey: ['picker', project, field.to, q],
		queryFn: () => searchEntries(project, field.to, q),
		enabled: searching && !full,
		staleTime: 5_000
	}));
	const results = $derived((found.data?.data ?? []).slice(0, 8));

	function set(next: string[]) {
		value = field.many ? next : (next[0] ?? '');
		onchange();
	}
	function choose(id: string) {
		if (chosen.includes(id)) return;
		set(field.many ? [...chosen, id] : [id]);
		typed = '';
		searching = field.many && chosen.length + 1 < (field.many ? field.max : 1);
	}
	function move(index: number, by: number) {
		const next = [...chosen];
		const [item] = next.splice(index, 1);
		if (item !== undefined) next.splice(index + by, 0, item);
		set(next);
	}
</script>

<div class="grid gap-2" data-testid="reference-{field.id}">
	{#if chosen.length > 0}
		<ol class="grid gap-1.5" aria-label="{field.label}: chosen">
			{#each named as item, index (item.id)}
				<li
					class="flex min-w-0 items-center gap-2 rounded-lg border bg-card px-3 py-1.5 text-sm {refusedAt === index ? 'border-destructive' : 'border-border'}"
					data-testid="reference-chip"
					data-invalid={refusedAt === index || undefined}
				>
					<LinkIcon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
					<span class="min-w-0 flex-1 truncate" class:text-muted-foreground={item.read !== undefined && !item.read.seen} data-user-content>{label(item.read, item.failed)}</span>
					{#if refusedAt === index}<span class="text-xs text-destructive" title={refusal}>Not found</span>{/if}
					{#if field.many && !disabled}
						<Button variant="ghost" size="icon-xs" aria-label="Move {label(item.read, item.failed)} up" disabled={index === 0} onclick={() => move(index, -1)}><ArrowUpIcon /></Button>
						<Button variant="ghost" size="icon-xs" aria-label="Move {label(item.read, item.failed)} down" disabled={index === chosen.length - 1} onclick={() => move(index, 1)}><ArrowDownIcon /></Button>
					{/if}
					{#if !disabled}
						<Button variant="ghost" size="icon-xs" aria-label="Remove {label(item.read, item.failed)}" onclick={() => set(chosen.filter((id) => id !== item.id))}><XIcon /></Button>
					{/if}
				</li>
			{/each}
		</ol>
	{/if}

	{#if !disabled}
		{#if full}
			<p class="text-xs text-muted-foreground">
				{#if field.many}At most {field.max} {field.max === 1 ? 'entry' : 'entries'}: remove one to add another.{:else}Remove it to choose another.{/if}
			</p>
		{:else}
			<div class="grid max-w-xl gap-1.5">
				<Input
					id={input}
					type="search"
					autocomplete="off"
					placeholder="Search {target} by title"
					aria-describedby={describedBy}
					bind:value={typed}
					onfocus={() => (searching = true)}
					oninput={() => (searching = true)}
					data-testid="reference-search"
				/>
				{#if searching}
					<div class="rounded-lg border border-border bg-card" data-testid="reference-results">
						{#if found.isPending}
							<p class="px-3 py-2 text-xs text-muted-foreground">Searching…</p>
						{:else if found.isError}
							<p class="px-3 py-2 text-xs text-destructive">
								{found.error instanceof ApiError && found.error.status === 403 ? `Your role can't see ${target}, so you can't choose one here.` : "Couldn't search just now. Try again."}
							</p>
						{:else if results.length === 0}
							<p class="px-3 py-2 text-xs text-muted-foreground">{q === '' ? `No ${target.toLowerCase()} yet.` : `No titles match “${q}”.`}</p>
						{:else}
							<ul aria-label="Matching {target}">
								{#each results as entry (entry.id)}
									<li class="rule-dotted last:border-b-0">
										<button
											type="button"
											class="flex w-full items-center gap-2 px-3 py-2 text-left text-sm outline-none hover:bg-muted focus-visible:bg-muted disabled:opacity-60"
											disabled={chosen.includes(entry.id)}
											onclick={() => choose(entry.id)}
										>
											<span class="min-w-0 flex-1 truncate" data-user-content>{shownTitle(entry.revision.title)}</span>
											{#if chosen.includes(entry.id)}<span class="text-xs text-muted-foreground">Chosen</span>{:else}<StatusBadge state={entryState(entry)} />{/if}
										</button>
									</li>
								{/each}
							</ul>
						{/if}
					</div>
				{/if}
			</div>
		{/if}
	{/if}
</div>
