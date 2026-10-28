<!--
  ⌘K: go anywhere in the project, or find an entry by title. Titles are searched on the server
  (the same `q` the collection pages use), across every collection, as you type.
-->
<script lang="ts">
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import LayoutDashboardIcon from '@lucide/svelte/icons/layout-dashboard';
	import MoonIcon from '@lucide/svelte/icons/moon';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import Settings2Icon from '@lucide/svelte/icons/settings-2';
	import UsersIcon from '@lucide/svelte/icons/users';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { toggleMode } from 'mode-watcher';
	import * as Command from '@colophon/ui/ui/command';
	import { type ContentType, entryState, searchEntries } from '#lib/api.ts';
	import StatusBadge from './StatusBadge.svelte';

	let {
		project,
		types,
		settings = false,
		open = $bindable(false)
	}: { project: string; types: ContentType[]; /** Whether the role sees Settings. */ settings?: boolean; open?: boolean } = $props();

	let typed = $state('');
	let q = $state('');
	// Wait for a pause in typing before asking the server.
	$effect(() => {
		const next = typed.trim();
		const timer = setTimeout(() => (q = next), 150);
		return () => clearTimeout(timer);
	});
	const found = createQuery(() => ({
		queryKey: ['palette', project, q],
		queryFn: async () => {
			// Each collection answers on its own: one that fails leaves the others' results, and says so.
			const pages = await Promise.allSettled(types.map(async (type) => ({ type, page: await searchEntries(project, type.slug, q) })));
			const answered = pages.flatMap((result) => (result.status === 'fulfilled' ? [result.value] : []));
			if (answered.length === 0 && pages.length > 0) throw (pages[0] as PromiseRejectedResult).reason;
			return {
				entries: answered.flatMap(({ type, page }) => page.data.slice(0, 8).map((entry) => ({ type, entry }))),
				missed: types.filter((_, index) => pages[index]?.status === 'rejected')
			};
		},
		enabled: open && q.length > 0 && types.length > 0,
		staleTime: 2_000
	}));

	function go(href: string) {
		open = false;
		typed = '';
		void goto(href);
	}

	function onKey(event: KeyboardEvent) {
		if ((event.metaKey || event.ctrlKey) && event.key.toLowerCase() === 'k') {
			event.preventDefault();
			open = !open;
		}
	}

	const PAGES = $derived([
		{ label: 'Overview', icon: LayoutDashboardIcon, href: resolve('/projects/[project]', { project }) },
		{ label: 'Publish requests', icon: LayersIcon, href: resolve('/projects/[project]/changesets', { project }) },
		{ label: 'Releases', icon: PackageCheckIcon, href: resolve('/projects/[project]/releases', { project }) },
		{ label: 'Members', icon: UsersIcon, href: resolve('/projects/[project]/members', { project }) },
		...(settings ? [{ label: 'Settings', icon: Settings2Icon, href: resolve('/projects/[project]/settings', { project }) }] : [])
	]);
	const needle = $derived(typed.trim().toLowerCase());
	const pages = $derived(PAGES.filter((item) => item.label.toLowerCase().includes(needle)));
	const collections = $derived(types.filter((type) => type.definition.name.toLowerCase().includes(needle)));
</script>

<svelte:window onkeydown={onKey} />

<Command.Dialog bind:open title="Search" description="Go to a page or find an entry by its title" shouldFilter={false}>
	<Command.Input placeholder="Search entries or jump to…" bind:value={typed} />
	<Command.List>
		<Command.Empty>{found.isFetching ? 'Searching…' : found.isError ? "Couldn't search just now. Try again in a moment." : 'Nothing matches.'}</Command.Empty>
		{#if found.data && found.data.missed.length > 0}
			<p class="px-3 py-2 text-xs text-muted-foreground" role="status" data-testid="search-missed">
				Couldn't search {found.data.missed.map((type) => type.definition.name).join(', ')} just now.
			</p>
		{/if}
		{#if found.data && found.data.entries.length > 0}
			<Command.Group heading="Entries">
				{#each found.data.entries as { type, entry } (entry.id)}
					<Command.Item
						value={`entry-${entry.id}`}
						onSelect={() => go(resolve('/projects/[project]/[collection]/[entry]', { project, collection: type.slug, entry: entry.id }))}
					>
						<FileTextIcon class="text-muted-foreground" />
						<span class="truncate">{entry.revision.title}</span>
						<span class="ml-auto flex items-center gap-2">
							<span class="text-xs text-muted-foreground">{type.definition.name}</span>
							<StatusBadge state={entryState(entry)} />
						</span>
					</Command.Item>
				{/each}
			</Command.Group>
		{/if}
		{#if pages.length + collections.length > 0}
		<Command.Group heading="Go to">
			{#each pages as item (item.label)}
				<Command.Item value={item.label} onSelect={() => go(item.href)}><item.icon class="text-muted-foreground" />{item.label}</Command.Item>
			{/each}
			{#each collections as type (type.slug)}
				<Command.Item value={`type-${type.slug}`} onSelect={() => go(resolve('/projects/[project]/[collection]', { project, collection: type.slug }))}>
					<FileTextIcon class="text-muted-foreground" />{type.definition.name}
				</Command.Item>
			{/each}
		</Command.Group>
		{/if}
		{#if 'switch theme'.includes(needle) || 'dark light'.includes(needle)}
			<Command.Group heading="Actions">
				<Command.Item value="theme" onSelect={() => { toggleMode(); open = false; }}><MoonIcon class="text-muted-foreground" />Switch light / dark</Command.Item>
			</Command.Group>
		{/if}
	</Command.List>
</Command.Dialog>
