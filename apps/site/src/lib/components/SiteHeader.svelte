<!-- The bar on every page: the mark, the pages, appearance, GitHub; a sheet with the same on a phone. -->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import MenuIcon from '@lucide/svelte/icons/menu';
	import { page } from '$app/state';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as Sheet from '@colophon/ui/ui/sheet';
	import { DOCS, GITHUB, NAV } from '#lib/site.ts';
	import Mark from './Mark.svelte';
	import ThemeMenu from './ThemeMenu.svelte';

	let menu = $state(false);
	const current = (href: string) => page.url.pathname === href || page.url.pathname.startsWith(href);
</script>

<a
	href="#content"
	class="sr-only z-50 rounded-full bg-primary px-4 py-2 text-sm text-primary-foreground focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
>
	Skip to content
</a>
<header class="sticky top-0 z-40 border-b border-border/70 bg-background/85 backdrop-blur-md supports-[backdrop-filter]:bg-background/75">
	<div class="shell flex h-16 items-center gap-3">
		<a href="/" class="flex items-center gap-2.5 rounded-lg py-1 pr-1 font-semibold tracking-tight outline-none focus-visible:ring-4 focus-visible:ring-ring/25">
			<Mark size={22} />
			<span class="text-[1.0625rem]">Colophon</span>
			<span class="sr-only">, home</span>
		</a>
		<span class="hidden rounded-full border border-border bg-card px-2 py-0.5 text-xs text-muted-foreground sm:inline" title="v0.1 is not out yet">
			pre-release
		</span>

		<nav aria-label="Main" class="ml-6 hidden lg:block">
			<ul class="flex items-center gap-1">
				{#each NAV as link (link.href)}
					<li>
						<a
							href={link.href}
							aria-current={current(link.href) ? 'page' : undefined}
							class="rounded-full px-3 py-2 text-sm text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-4 focus-visible:ring-ring/25 aria-[current=page]:bg-card aria-[current=page]:text-foreground aria-[current=page]:shadow-hairline"
						>
							{link.label}
						</a>
					</li>
				{/each}
				<li>
					<a href={DOCS} rel="external" class="rounded-full px-3 py-2 text-sm text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-4 focus-visible:ring-ring/25">
						Docs
					</a>
				</li>
			</ul>
		</nav>

		<div class="ml-auto flex items-center gap-1.5">
			<ThemeMenu />
			<Button variant="outline" size="sm" href={GITHUB} class="hidden sm:inline-flex">
				GitHub<ArrowUpRightIcon class="size-3.5" aria-hidden="true" />
			</Button>
			<Sheet.Root bind:open={menu}>
				<Sheet.Trigger class="{buttonVariants({ variant: 'ghost', size: 'icon-sm' })} lg:hidden" aria-label="Menu">
					<MenuIcon />
				</Sheet.Trigger>
				<Sheet.Content side="right" class="w-[min(20rem,85vw)] gap-0 p-6">
					<Sheet.Header class="p-0">
						<Sheet.Title class="flex items-center gap-2.5"><Mark size={20} />Colophon</Sheet.Title>
						<Sheet.Description>Pre-release: v0.1 is not out yet.</Sheet.Description>
					</Sheet.Header>
					<nav aria-label="Main, on a small screen" class="mt-6">
						<ul class="grid gap-1">
							<li><a href="/" onclick={() => (menu = false)} class="block rounded-xl px-3 py-2.5 text-[0.9375rem] hover:bg-accent">Home</a></li>
							{#each NAV as link (link.href)}
								<li>
									<a
										href={link.href}
										onclick={() => (menu = false)}
										aria-current={current(link.href) ? 'page' : undefined}
										class="block rounded-xl px-3 py-2.5 text-[0.9375rem] hover:bg-accent aria-[current=page]:bg-accent aria-[current=page]:font-medium"
									>
										{link.label}
									</a>
								</li>
							{/each}
							<li><a href={DOCS} rel="external" class="block rounded-xl px-3 py-2.5 text-[0.9375rem] hover:bg-accent">Docs</a></li>
							<li><a href={GITHUB} class="block rounded-xl px-3 py-2.5 text-[0.9375rem] hover:bg-accent">GitHub</a></li>
						</ul>
					</nav>
				</Sheet.Content>
			</Sheet.Root>
		</div>
	</div>
</header>
