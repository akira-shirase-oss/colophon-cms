<!--
  Search, with Pagefind's index of the built site (npm run build writes it to /docs/pagefind/, on
  this site's own origin). The index and its code load the first time the dialog opens. ⌘K,
  Ctrl+K or / opens it from anywhere; Enter opens the first result.
-->
<script lang="ts">
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { goto } from '$app/navigation';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Kbd } from '@colophon/ui/ui/kbd';
	import { BASE, href } from '#lib/paths.ts';

	interface Result {
		url: string;
		excerpt: string;
		meta: { title?: string };
		sub_results?: { title: string; url: string; excerpt: string; locations?: number[] }[];
	}
	/** One line of results: a page, or the section of it that matched (a changelog entry, say). */
	interface Hit {
		url: string;
		page: string;
		section: string | null;
		excerpt: string;
	}
	interface Pagefind {
		options: (options: { baseUrl: string }) => Promise<void>;
		init: () => Promise<void>;
		debouncedSearch: (term: string, options?: object, wait?: number) => Promise<{ results: { data: () => Promise<Result> }[] } | null>;
	}

	let { open = $bindable(false) }: { open?: boolean } = $props();

	let query = $state('');
	let results = $state<Hit[]>([]);
	let status = $state<'idle' | 'loading' | 'ready' | 'unavailable'>('idle');
	let searched = $state('');
	let pagefind: Pagefind | undefined;

	async function load(): Promise<Pagefind | undefined> {
		if (pagefind) return pagefind;
		try {
			// Built by `npm run build`; absent under `vite dev`, where search says so.
			const url = href('/pagefind/pagefind.js');
			pagefind = (await import(/* @vite-ignore */ url)) as Pagefind;
			// The index names pages from build/'s root; the docs are served under the base path.
			await pagefind.options({ baseUrl: `${BASE}/` });
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
		results = data.map((result) => hit(result, term));
		searched = term;
		status = 'ready';
	}

	/**
	 * A long page (the changelog, a reference) answers with the heading the match is under: the one
	 * whose title has the most of the words searched for, then the one with the most matches.
	 */
	function hit(result: Result, term: string): Hit {
		const page = result.meta.title ?? result.url;
		const words = term.toLowerCase().split(/\s+/).filter(Boolean);
		const score = (sub: { title: string; locations?: number[] }) =>
			words.filter((word) => sub.title.toLowerCase().includes(word)).length * 1000 + (sub.locations?.length ?? 0);
		const best = (result.sub_results ?? [])
			.filter((sub) => sub.url.includes('#') && sub.title !== page)
			.sort((a, b) => score(b) - score(a))[0];
		return best
			? { url: best.url, page, section: best.title, excerpt: best.excerpt }
			: { url: result.url, page, section: null, excerpt: result.excerpt };
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
	<Dialog.Content class="top-[12vh] translate-y-0 gap-0 overflow-hidden rounded-[28px] border-0 p-0 shadow-[var(--shadow-float)] sm:max-w-xl" showCloseButton={false}>
		<Dialog.Title class="sr-only">Search the documentation</Dialog.Title>
		<Dialog.Description class="sr-only">Results update as you type. Enter opens the first one.</Dialog.Description>
		<div class="flex items-center gap-3 border-b border-[color:var(--line)] px-5">
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
					Type to search every page: guides, references, the changelog and the design documents.
				{/if}
			</p>
			{#if results.length > 0}
				<ul class="space-y-1" data-testid="search-results">
					{#each results as result (result.url)}
						<li>
							<a
								href={result.url}
								onclick={() => (open = false)}
								class="flex gap-3 rounded-2xl px-3 py-2.5 outline-none hover:bg-[var(--chip)] focus-visible:bg-[var(--chip)] focus-visible:ring-2 focus-visible:ring-ring"
							>
								<FileTextIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
								<span class="min-w-0">
									<span class="block font-medium"
										>{result.page}{#if result.section}<span class="font-normal text-muted-foreground">{' › '}{result.section}</span>{/if}</span
									>
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
