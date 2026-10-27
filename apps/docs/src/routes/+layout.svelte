<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { ModeWatcher } from 'mode-watcher';
	import mark from '#lib/colophon-mark.svg';
	import Footer from '#lib/components/Footer.svelte';
	import Header from '#lib/components/Header.svelte';
	import NavTree from '#lib/components/NavTree.svelte';
	import PreReleaseBanner from '#lib/components/PreReleaseBanner.svelte';
	import { href, within } from '#lib/paths.ts';
	import type { LayoutProps } from './$types';

	let { data, children }: LayoutProps = $props();

	const current = $derived(within(page.url.pathname));
	const home = $derived(current === '/');
</script>

<svelte:head>
	<link rel="icon" href={mark} />
	<link rel="alternate" type="application/atom+xml" title="Colophon changelog" href={href('/changelog/feed.xml')} />
</svelte:head>

<ModeWatcher defaultMode="system" />

<a
	href="#content"
	class="sr-only z-[60] rounded-full bg-primary px-4 py-2 text-sm font-medium text-primary-foreground focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
>
	Skip to the content
</a>

<PreReleaseBanner />
<Header {current} repo={data.repo} />

<div class="mx-auto flex w-full max-w-[90rem] {home ? '' : 'lg:gap-10'} px-4 sm:px-6">
	{#if !home}
		<aside class="hidden w-64 shrink-0 lg:block" data-pagefind-ignore>
			<div class="sticky top-[4.5rem] -ml-3 max-h-[calc(100svh-4.5rem)] overflow-y-auto py-8 pr-2">
				<NavTree {current} repo={data.repo} />
			</div>
		</aside>
	{/if}
	<main id="content" tabindex="-1" class="min-w-0 flex-1 py-8 outline-none sm:py-10 lg:py-12">
		{#key current}
			{@render children()}
		{/key}
	</main>
</div>

<Footer />
