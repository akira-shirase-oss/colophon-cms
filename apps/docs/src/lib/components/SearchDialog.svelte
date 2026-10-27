<!--
  Search, with Pagefind's index of the built site (npm run build writes it to /pagefind/, on
  this site's own origin). The index and its code load the first time the dialog opens. ⌘K,
  Ctrl+K or / opens it from anywhere; Enter opens the first result.
-->
<script lang="ts">
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { goto } from '$app/navigation';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Kbd } from '@colophon/ui/ui/kbd';

	interface Result {
		url: string;
		excerpt: string;
		meta: { title?: string };
	}
	interface Pagefind {
		init: () => Promise<void>;
		debouncedSearch: (term: string, options?: object, wait?: number) => Promise<{ results: { data: () => Promise<Result> }[] } | null>;
	}

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let query = $state('');
	let results = $state<Result[]>([]);
	let status = $state<'idle' | 'loading' | 'ready' | 'unavailable'>('idle');
	let searched = $state('');
	let pagefind: Pagefind | undefined;

	async function load(): Promise<Pagefind | undefined> {
		if (pagefind) return pagefind;
		try {
			// Built by `npm run build`; absent under `vite dev`, where search says so.
			const url = '/pagefind/pagefind.js';
			pagefind = (await import(/* @vite-ignore */ url)) as Pagefind;
			await pagefind.init();
			return pagefind;
		} catch {
			status = 'unavailable';
			return undefined;
		}
	}

	async function run(term: string) {
		const engine = await load();
		if (!engine) return;
		if (term.trim() === '') {
			results = [];
			searched = '';
			status = 'idle';
			return;
		}
		status = 'loading';
		const found = await engine.debouncedSearch(term, {}, 120);
		// A newer query superseded this one.
		if (found === null) return;
		const data = await Promise.all(found.results.slice(0, 8).map((result) => result.data()));
		if (term !== query) return;
		results = data;
		searched = term;
		status = 'ready';
	}

	$effect(() => {
		if (open) void run(query);
	});

	function openFirst(event: KeyboardEvent) {
		const first = results[0];
		if (event.key === 'Enter' && first) {
			event.preventDefault();
			open = false;
			void goto(first.url);
		}
	}

	function shortcut(event: KeyboardEvent) {
		const target = event.target as HTMLElement | null;
		const typing = target?.closest('input, textarea, select, [contenteditable="true"]') !== null && target !== null;
		if ((event.key === 'k' && (event.metaKey || event.ctrlKey)) || (event.key === '/' && !typing)) {
			event.preventDefault();
			open = true;
		}
	}
</script>

<svelte:window onkeydown={shortcut} />

<Dialog.Root bind:open>
	<Dialog.Content class="top-[12vh] translate-y-0 gap-0 p-0 sm:max-w-xl" showCloseButton={false}>
		<Dialog.Title class="sr-only">Search the documentation</Dialog.Title>
		<Dialog.Description class="sr-only">Results update as you type. Enter opens the first one.</Dialog.Description>
		<div class="flex items-center gap-3 border-b border-border px-4">
			<SearchIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
			<input
				bind:value={query}
				onkeydown={openFirst}
				type="search"
				aria-label="Search the documentation"
				placeholder="Search guides, concepts and references"
				class="h-14 w-full min-w-0 bg-transparent text-base outline-none placeholder:text-muted-foreground md:text-sm"
				data-testid="search-input"
			/>
			<Kbd>Esc</Kbd>
		</div>
		<div class="max-h-[60vh] overflow-y-auto p-2">
			<p class="px-3 py-2 text-xs text-muted-foreground" role="status" data-testid="search-status">
				{#if status === 'unavailable'}
					The search index is written by <code class="font-mono">npm run build</code>; it isn't there in development.
				{:else if status === 'loading'}
					Searching…
				{:else if status === 'ready' && results.length === 0}
					Nothing matches “{searched}”.
				{:else if status === 'ready'}
					{results.length === 8 ? 'Top 8 results' : `${results.length} ${results.length === 1 ? 'result' : 'results'}`} for “{searched}”
				{:else}
					Type to search every page, the design documents included.
				{/if}
			</p>
			{#if results.length > 0}
				<ul class="space-y-1" data-testid="search-results">
					{#each results as result (result.url)}
						<li>
							<a
								href={result.url}
								onclick={() => (open = false)}
								class="flex gap-3 rounded-xl px-3 py-2.5 outline-none hover:bg-accent focus-visible:bg-accent focus-visible:ring-[3px] focus-visible:ring-ring/40"
							>
								<FileTextIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
								<span class="min-w-0">
									<span class="block font-medium">{result.meta.title ?? result.url}</span>
									<!-- Pagefind's excerpt: escaped text from this site's own pages, with <mark> around matches. -->
									<span class="mt-0.5 line-clamp-2 block text-[0.8125rem] text-muted-foreground [&_mark]:bg-brand-soft [&_mark]:text-brand-ink">{@html result.excerpt}</span>
								</span>
							</a>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
	</Dialog.Content>
</Dialog.Root>
