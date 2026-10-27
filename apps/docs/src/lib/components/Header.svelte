<!--
  The bar across every page: the mark, "pre-release", search, the repository, the appearance
  choice, and on narrow screens a menu button that opens the navigation in a sheet.
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { afterNavigate } from '$app/navigation';
	import { buttonVariants } from '@colophon/ui/ui/button';
	import { Kbd } from '@colophon/ui/ui/kbd';
	import * as Sheet from '@colophon/ui/ui/sheet';
	import { cn } from '@colophon/ui/utils';
	import { REPOSITORY } from '#lib/nav.ts';
	import type { DesignDocLink } from '#lib/types.ts';
	import AppearanceMenu from './AppearanceMenu.svelte';
	import Mark from './Mark.svelte';
	import NavTree from './NavTree.svelte';
	import SearchDialog from './SearchDialog.svelte';

	let { current, design }: { current: string; design: DesignDocLink[] } = $props();

	let searching = $state(false);
	let menu = $state(false);
	afterNavigate(() => (menu = false));
</script>

<header class="sticky top-0 z-40 border-b border-border bg-background/95 backdrop-blur supports-[backdrop-filter]:bg-background/80">
	<div class="mx-auto flex h-16 max-w-[90rem] items-center gap-2 px-4 sm:gap-3 sm:px-6">
		<Sheet.Root bind:open={menu}>
			<Sheet.Trigger class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }), '-ml-1.5 lg:hidden')} aria-label="Open the navigation">
				<MenuIcon />
			</Sheet.Trigger>
			<Sheet.Content side="left" class="w-[88vw] max-w-sm overflow-y-auto bg-background p-5 pt-6">
				<Sheet.Title class="mb-5 flex items-center gap-2 text-sm font-semibold"><Mark size={18} />Colophon docs</Sheet.Title>
				<Sheet.Description class="sr-only">Every page of the documentation, by section.</Sheet.Description>
				<NavTree {current} {design} />
			</Sheet.Content>
		</Sheet.Root>

		<a href="/" class="flex shrink-0 items-center gap-2.5 rounded-md font-semibold tracking-tight whitespace-nowrap outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40">
			<Mark />
			<span>Colophon <span class="font-normal text-muted-foreground">docs</span></span>
		</a>
		<span
			class="hidden rounded-full border border-border bg-card px-2 py-0.5 text-xs text-muted-foreground sm:inline"
			title="Colophon is pre-release: v0.1 is not out"
		>
			pre-release
		</span>

		<div class="ml-auto flex items-center gap-1 sm:gap-2">
			<button
				type="button"
				onclick={() => (searching = true)}
				class={cn(buttonVariants({ variant: 'outline', size: 'sm' }), 'hidden w-64 justify-start gap-2 text-muted-foreground shadow-hairline md:inline-flex')}
				data-testid="search-open"
			>
				<SearchIcon aria-hidden="true" />
				<span class="flex-1 text-left">Search the docs</span>
				<Kbd>⌘K</Kbd>
			</button>
			<button
				type="button"
				onclick={() => (searching = true)}
				class={cn(buttonVariants({ variant: 'ghost', size: 'icon-sm' }), 'md:hidden')}
				aria-label="Search the docs"
			>
				<SearchIcon />
			</button>
			<a href={REPOSITORY} class={cn(buttonVariants({ variant: 'ghost', size: 'sm' }), 'hidden sm:inline-flex')} rel="external">
				GitHub <ArrowUpRightIcon aria-hidden="true" class="text-muted-foreground" />
			</a>
			<AppearanceMenu />
		</div>
	</div>
</header>

<SearchDialog bind:open={searching} />
