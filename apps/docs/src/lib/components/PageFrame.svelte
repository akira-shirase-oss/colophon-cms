<!--
  Every documentation page: the section it is in, its one h1, its capability label, the table of
  contents (beside the text on wide screens, folded above it on narrow ones), the text, then an
  edit link and the previous and next pages. Code blocks' copy buttons are handled here, by one
  listener for the whole page, because the blocks are HTML written at build time.
-->
<script lang="ts">
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import SquarePenIcon from '@lucide/svelte/icons/square-pen';
	import type { Snippet } from 'svelte';
	import { page } from '$app/state';
	import type { TocEntry } from '#lib/markdown/structure.ts';
	import { type Capability, editUrl, neighbours, SECTIONS } from '#lib/nav.ts';
	import CapabilityLabel from './CapabilityLabel.svelte';
	import Toc from './Toc.svelte';

	let {
		title,
		description,
		capability,
		source,
		toc = [],
		section,
		note,
		wide = false,
		children
	}: {
		title: string;
		description?: string;
		capability: Capability;
		source: string;
		toc?: TocEntry[];
		/** The section's name above the title; found from the site map when not given. */
		section?: string;
		/** A line under the label, such as what a design document is. */
		note?: Snippet;
		/** Use the whole column: for the design documents, whose tables have many wide columns. */
		wide?: boolean;
		children: Snippet;
	} = $props();

	const here = $derived(page.url.pathname);
	const order = $derived((page.data['order'] as { href: string; title: string }[] | undefined) ?? []);
	const around = $derived(neighbours(order, here));
	const sectionTitle = $derived(section ?? SECTIONS.find((candidate) => candidate.pages.some((item) => item.href === here))?.title);

	let body = $state<HTMLElement | null>(null);
	let announcement = $state('');

	$effect(() => {
		const root = body;
		if (!root) return;
		let timer: ReturnType<typeof setTimeout> | undefined;
		const onClick = (event: MouseEvent) => {
			const button = event.target instanceof Element ? event.target.closest('[data-copy]') : null;
			if (!(button instanceof HTMLElement)) return;
			const code = button.closest('[data-code-block]')?.querySelector('pre')?.textContent ?? '';
			navigator.clipboard.writeText(code).then(
				() => {
					button.dataset['copied'] = '';
					announcement = 'Copied to the clipboard';
					clearTimeout(timer);
					timer = setTimeout(() => {
						delete button.dataset['copied'];
						announcement = '';
					}, 2000);
				},
				() => (announcement = 'Copying failed: select the code and copy it instead')
			);
		};
		root.addEventListener('click', onClick);
		// Tables scroll sideways when they don't fit; only then do they take a tab stop.
		const tables = [...root.querySelectorAll<HTMLElement>('.table-scroll')];
		const fit = () => {
			for (const table of tables) {
				if (table.scrollWidth > table.clientWidth + 1) table.tabIndex = 0;
				else table.removeAttribute('tabindex');
			}
		};
		fit();
		const observer = new ResizeObserver(fit);
		for (const table of tables) observer.observe(table);
		return () => {
			root.removeEventListener('click', onClick);
			observer.disconnect();
			clearTimeout(timer);
		};
	});
</script>

<svelte:head>
	<title>{here === '/' ? 'Colophon documentation' : `${title} · Colophon docs`}</title>
	{#if description}<meta name="description" content={description} />{/if}
</svelte:head>

<div class="grid gap-x-12 xl:grid-cols-[minmax(0,1fr)_13.5rem]">
	<article class="page-enter min-w-0 {wide ? 'max-w-none' : 'max-w-[46rem]'}" data-pagefind-body>
		<header class="mb-8 space-y-3">
			{#if sectionTitle}<p class="text-sm font-medium text-brand-ink" data-pagefind-ignore>{sectionTitle}</p>{/if}
			<h1 class="display-2" data-pagefind-meta="title">{title}</h1>
			{#if description}<p class="text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">{description}</p>{/if}
			<div class="flex flex-wrap items-center gap-x-3 gap-y-2 pt-1" data-pagefind-ignore>
				<CapabilityLabel {capability} />
				{#if note}<span class="text-sm text-muted-foreground">{@render note()}</span>{/if}
			</div>
		</header>

		{#if toc.length > 1}
			<details class="mb-8 rounded-2xl border border-border bg-card px-4 py-3 shadow-hairline xl:hidden" data-pagefind-ignore>
				<summary class="cursor-pointer text-sm font-medium outline-none select-none focus-visible:ring-[3px] focus-visible:ring-ring/40">On this page</summary>
				<div class="mt-3"><Toc entries={toc} label="On this page (folded)" /></div>
			</details>
		{/if}

		<div class="prose-docs {wide ? 'prose-wide' : ''}" bind:this={body}>
			{@render children()}
		</div>

		<footer class="mt-14 space-y-6 border-t border-border pt-6" data-pagefind-ignore>
			<a
				href={editUrl(source)}
				class="inline-flex items-center gap-1.5 rounded-md text-sm text-muted-foreground underline-offset-4 outline-none hover:text-foreground hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/40"
				rel="external"
			>
				<SquarePenIcon class="size-4" aria-hidden="true" />Edit this page on GitHub
			</a>
			{#if around.previous || around.next}
				<nav aria-label="Previous and next pages" class="grid gap-3 sm:grid-cols-2">
					{#if around.previous}
						<a
							href={around.previous.href}
							rel="prev"
							class="group rounded-2xl border border-border bg-card p-4 shadow-hairline outline-none hover:shadow-soft focus-visible:ring-[3px] focus-visible:ring-ring/40"
						>
							<span class="flex items-center gap-1.5 text-xs text-muted-foreground"><ArrowLeftIcon class="size-3.5" aria-hidden="true" />Previous</span>
							<span class="mt-1 block font-medium">{around.previous.title}</span>
						</a>
					{:else}
						<span class="hidden sm:block"></span>
					{/if}
					{#if around.next}
						<a
							href={around.next.href}
							rel="next"
							class="group rounded-2xl border border-border bg-card p-4 text-right shadow-hairline outline-none hover:shadow-soft focus-visible:ring-[3px] focus-visible:ring-ring/40"
						>
							<span class="flex items-center justify-end gap-1.5 text-xs text-muted-foreground">Next<ArrowRightIcon class="size-3.5" aria-hidden="true" /></span>
							<span class="mt-1 block font-medium">{around.next.title}</span>
						</a>
					{/if}
				</nav>
			{/if}
		</footer>
		<p class="sr-only" role="status">{announcement}</p>
	</article>

	{#if toc.length > 1}
		<aside class="hidden xl:block" data-pagefind-ignore>
			<div class="sticky top-24 max-h-[calc(100svh-7rem)] overflow-y-auto pb-8">
				<Toc entries={toc} />
			</div>
		</aside>
	{/if}
</div>
