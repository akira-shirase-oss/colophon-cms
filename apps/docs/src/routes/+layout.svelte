<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { ModeWatcher } from 'mode-watcher';
	import mark from '#lib/colophon-mark.svg';
	import Header from '#lib/components/Header.svelte';
	import NavTree from '#lib/components/NavTree.svelte';
	import { REPOSITORY } from '#lib/nav.ts';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	const current = $derived(page.url.pathname);
	const home = $derived(current === '/');
</script>

<svelte:head>
	<link rel="icon" href={mark} />
</svelte:head>

<ModeWatcher defaultMode="system" />

<a
	href="#content"
	class="sr-only z-50 rounded-full bg-primary px-4 py-2 text-sm font-medium text-primary-foreground focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
>
	Skip to the content
</a>

<Header {current} design={data.design} />

<div class="mx-auto flex w-full max-w-[90rem] {home ? '' : 'lg:gap-10'} px-4 sm:px-6">
	{#if !home}
		<aside class="hidden w-60 shrink-0 lg:block" data-pagefind-ignore>
			<div class="sticky top-16 -ml-2.5 max-h-[calc(100svh-4rem)] overflow-y-auto py-8 pr-2">
				<NavTree {current} design={data.design} />
			</div>
		</aside>
	{/if}
	<main id="content" tabindex="-1" class="min-w-0 flex-1 py-8 outline-none sm:py-10 lg:py-12">
		{#key current}
			{@render children()}
		{/key}
	</main>
</div>

<footer class="border-t border-border" data-pagefind-ignore>
	<div class="mx-auto flex max-w-[90rem] flex-col gap-2 px-4 py-8 text-sm text-muted-foreground sm:flex-row sm:items-center sm:justify-between sm:px-6">
		<p>
			Colophon is pre-release: v0.1 is not out. Labels describe the code at commit
			<a class="font-mono text-foreground underline decoration-border underline-offset-4 hover:decoration-foreground" href="{REPOSITORY}/commit/{__COMMIT__}" rel="external">{__COMMIT__}</a>.
		</p>
		<p>Apache-2.0 is proposed, not yet confirmed (O-014).</p>
	</div>
</footer>
