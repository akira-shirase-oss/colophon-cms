<script lang="ts">
	import '../app.css';
	import { ModeWatcher } from 'mode-watcher';
	import * as Tooltip from '@colophon/ui/ui/tooltip';
	import SiteFooter from '#lib/components/SiteFooter.svelte';
	import SiteHeader from '#lib/components/SiteHeader.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="alternate" type="application/rss+xml" title="Colophon build log" href="/blog/rss.xml" />
</svelte:head>

<ModeWatcher defaultMode="system" />
<Tooltip.Provider delayDuration={250}>
	<div class="page-ground relative flex min-h-svh flex-col overflow-x-clip">
		<SiteHeader />
		<main id="content" tabindex="-1" class="flex-1 outline-none">
			{@render children()}
		</main>
		<SiteFooter />
	</div>
</Tooltip.Provider>
