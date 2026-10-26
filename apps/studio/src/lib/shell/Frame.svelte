<!--
  The frame for pages outside a project (projects, sign-in, device approval, invitations, the
  playground): the warm-grey ground, a slim bar with the mark, and one white page.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import AppearanceMenu from './AppearanceMenu.svelte';
	import Mark from './Mark.svelte';

	let { children }: { children: Snippet } = $props();
</script>

<div class="flex min-h-svh flex-col">
	<header class="flex h-14 items-center gap-3 px-4 sm:px-6">
		<a href={resolve('/')} class="flex items-center gap-2.5 rounded-md font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40">
			<Mark />
			<span>Colophon Studio</span>
		</a>
		<span class="rounded-full border border-border bg-card px-2 py-0.5 text-xs text-muted-foreground" title="Colophon is not released yet">preview</span>
		<div class="ml-auto"><AppearanceMenu /></div>
	</header>
	<main class="mx-2 mb-2 flex-1 rounded-2xl border border-sidebar-border bg-background shadow-xs">
		{#key page.url.pathname}
			<div class="page-enter mx-auto w-full max-w-6xl px-5 py-8 sm:px-10 sm:py-12">
				{@render children()}
			</div>
		{/key}
	</main>
</div>
