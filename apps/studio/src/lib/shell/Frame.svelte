<!--
  The frame for pages outside a project (projects, sign-in, device approval, invitations, the
  playground): the warm-grey ground, a slim bar with the mark, and one white page.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { Badge } from '@colophon/ui/ui/badge';
	import AccountMenu from './AccountMenu.svelte';
	import ThemeToggle from './ThemeToggle.svelte';
	import Mark from './Mark.svelte';

	let { children }: { children: Snippet } = $props();
</script>

<div class="flex min-h-svh flex-col">
	<header class="flex h-14 items-center gap-3 px-4 sm:px-6">
		<a href={resolve('/')} class="flex items-center gap-2.5 rounded-md font-medium outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40">
			<Mark />
			<span class="font-title font-normal" data-testid="wordmark">Colophon Studio</span>
		</a>
		<Badge variant="outline" class="text-muted-foreground" title="Colophon is not released yet">preview</Badge>
		<div class="ml-auto flex items-center gap-1"><ThemeToggle /><AccountMenu /></div>
	</header>
	<main class="mx-2 mb-2 flex-1 rounded-2xl border border-sidebar-border bg-background">
		{#key page.url.pathname}
			<div class="page-enter mx-auto w-full max-w-6xl px-5 py-8 sm:px-10 sm:py-12">
				{@render children()}
			</div>
		{/key}
	</main>
</div>
