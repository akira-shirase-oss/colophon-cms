<!--
  The bar across every page, in the website's look (apps/site SiteHeader): a glass pill with the
  mark, which leads to the website, the "Docs" label, which leads to the docs' front page, the
  sections as tabs, search, the website, GitHub and the one-button theme toggle. Below 1,200px the
  tabs fold into a menu sheet with the whole navigation.
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import SearchIcon from '@lucide/svelte/icons/search';
	import { afterNavigate } from '$app/navigation';
	import { Kbd } from '@colophon/ui/ui/kbd';
	import * as Sheet from '@colophon/ui/ui/sheet';
	import { REPOSITORY, SECTIONS } from '#lib/nav.ts';
	import { href, site } from '#lib/paths.ts';
	import type { RepoLinks } from '#lib/types.ts';
	import BrandIcon from './BrandIcon.svelte';
	import Mark from './Mark.svelte';
	import NavTree from './NavTree.svelte';
	import SearchDialog from './SearchDialog.svelte';
	import ThemeToggle from './ThemeToggle.svelte';

	let { current, repo }: { current: string; repo: RepoLinks } = $props();

	let searching = $state(false);
	let menu = $state(false);
	afterNavigate(() => (menu = false));

	const TABS = SECTIONS.map((section) => ({
		label: section.short,
		href: section.pages[0]?.href ?? '/',
		prefixes: [...new Set(section.pages.map((page) => `/${page.href.split('/')[1] ?? ''}/`))]
	}));
	const active = (tab: (typeof TABS)[number]) => current !== '/' && tab.prefixes.some((prefix) => prefix !== '//' && current.startsWith(prefix));
</script>

<header class="sticky top-0 z-40 bg-background/85 px-2 pt-2 pb-2 backdrop-blur-md sm:px-4" data-pagefind-ignore>
	<div class="bar relative mx-auto flex h-14 max-w-[90rem] items-center gap-1 rounded-[40px] px-2 sm:gap-2">
		<Sheet.Root bind:open={menu}>
			<Sheet.Trigger
				class="grid size-10 shrink-0 place-items-center rounded-full outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring lg:hidden"
				aria-label="Open the navigation"
			>
				<MenuIcon class="size-5" aria-hidden="true" />
			</Sheet.Trigger>
			<Sheet.Content side="left" class="w-[88vw] max-w-sm overflow-y-auto border-0 bg-popover p-5 pt-6 shadow-[var(--shadow-float)]">
				<Sheet.Title class="mb-5 flex items-center gap-2 text-lg font-semibold"><Mark size={20} />Colophon docs</Sheet.Title>
				<Sheet.Description class="sr-only">Every page of the documentation, by section.</Sheet.Description>
				<NavTree {current} {repo} />
				<div class="mt-6 grid gap-1 border-t border-[color:var(--line)] pt-4 text-[0.9375rem]">
					<a href={site('/')} rel="external" class="rounded-2xl px-3 py-2 hover:bg-accent">The website</a>
					<a href={REPOSITORY} rel="external" class="rounded-2xl px-3 py-2 hover:bg-accent">GitHub</a>
				</div>
			</Sheet.Content>
		</Sheet.Root>

		<div class="flex shrink-0 items-center gap-1">
			<a
				href={site('/')}
				rel="external"
				class="flex items-center gap-1.5 rounded-full py-1 pr-1.5 pl-2 text-lg leading-6 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring"
			>
				<Mark size={20} />
				<!-- Below 360px the bar keeps the mark and "Docs"; the name stays for screen readers. -->
				<span class="max-[359px]:sr-only">Colophon</span>
				<span class="sr-only">, the website</span>
			</a>
			<a
				href={href('/')}
				class="docs-chip rounded-full px-2.5 py-1 text-xs leading-4 font-semibold tracking-wide uppercase outline-none focus-visible:ring-2 focus-visible:ring-ring"
				aria-current={current === '/' ? 'page' : undefined}
			>
				Docs<span class="sr-only">, front page</span>
			</a>
		</div>

		<nav aria-label="Sections" class="ml-3 hidden min-w-0 xl:block">
			<ul class="flex items-center gap-0.5">
				{#each TABS as tab (tab.href)}
					<li>
						<a
							href={href(tab.href)}
							aria-current={active(tab) ? 'true' : undefined}
							class="tab block rounded-full px-3 py-1.5 text-[0.9375rem] leading-6 text-ink-2 outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring"
							>{tab.label}</a
						>
					</li>
				{/each}
			</ul>
		</nav>

		<div class="ml-auto flex items-center gap-1 sm:gap-1.5">
			<button
				type="button"
				onclick={() => (searching = true)}
				class="search hidden h-10 w-56 items-center gap-2 rounded-full pr-1.5 pl-3.5 text-[0.9375rem] text-muted-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring md:flex lg:w-64"
				data-testid="search-open"
			>
				<SearchIcon class="size-4" aria-hidden="true" />
				<span class="flex-1 text-left">Search the docs</span>
				<Kbd class="bg-background">⌘K</Kbd>
			</button>
			<button
				type="button"
				onclick={() => (searching = true)}
				class="grid size-10 place-items-center rounded-full outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring md:hidden"
				aria-label="Search the docs"
			>
				<SearchIcon class="size-[18px]" aria-hidden="true" />
			</button>
			<a
				href={site('/')}
				rel="external"
				class="hidden items-center gap-1 rounded-full px-3 py-2 text-[0.9375rem] leading-6 text-foreground outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring dt:inline-flex"
			>
				Website<ArrowUpRightIcon class="size-4 text-muted-foreground" aria-hidden="true" />
			</a>
			<a
				href={REPOSITORY}
				rel="external"
				class="hidden size-10 place-items-center rounded-full text-foreground outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring sm:grid"
				aria-label="Colophon on GitHub"
			>
				<BrandIcon name="github" size={19} />
			</a>
			<ThemeToggle />
		</div>
	</div>
</header>

<SearchDialog bind:open={searching} />

<style>
	/* The website's glass pill (apps/site SiteHeader), always on: the docs scroll under it. */
	.bar {
		background: var(--nav-glass);
		box-shadow: inset 0 0 0 1px var(--nav-edge);
	}
	.docs-chip {
		background: var(--chip);
		color: var(--eyebrow);
	}
	.docs-chip:hover,
	.docs-chip[aria-current='page'] {
		background: var(--chip-hover);
	}
	.tab {
		transition:
			background-color 200ms ease,
			color 200ms ease;
	}
	.tab:hover,
	.tab[aria-current='true'] {
		background: var(--tertiary-hover);
	}
	.tab[aria-current='true'] {
		color: var(--foreground);
		font-weight: 500;
		background: var(--chip);
	}
	.search {
		background: var(--chip);
		transition: background-color 200ms ease;
	}
	.search:hover {
		background: var(--chip-hover);
	}
</style>
