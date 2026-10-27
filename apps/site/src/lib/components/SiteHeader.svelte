<!--
  The bar on every page, after the reference's: on a desktop the mark, the pages and the main
  button float over the page; once the page scrolls, a glass pill fades in behind them and the
  ends draw in (transform and opacity only). Below 1,200px it is a glass pill with the mark and
  a menu, which opens as a sheet.
-->
<script lang="ts">
	import MenuIcon from '@lucide/svelte/icons/menu';
	import { page } from '$app/state';
	import * as Sheet from '@colophon/ui/ui/sheet';
	import { GITHUB, NAV, QUICKSTART } from '#lib/site.ts';
	import BrandIcon from './BrandIcon.svelte';
	import Mark from './Mark.svelte';
	import Pill from './Pill.svelte';
	import ThemeToggle from './ThemeToggle.svelte';

	let menu = $state(false);
	let scrolled = $state(false);
	let bar = $state<HTMLElement | null>(null);
	const current = (href: string) => page.url.pathname === href || page.url.pathname.startsWith(href);

	// How far the pill's ends draw in once scrolled: the reference goes from 1,040 to 834px wide;
	// with more links here it stops where the contents still fit.
	function measure() {
		if (!bar) return;
		const parts = [...bar.querySelectorAll<HTMLElement>('[data-nav-part]')];
		const used = parts.reduce((sum, part) => sum + part.offsetWidth, 0);
		const free = bar.clientWidth - used - 16 - 2 * 32;
		bar.style.setProperty('--nav-inset', `${Math.max(0, Math.min(103, free / 2 - 8)).toFixed(0)}px`);
	}

	$effect(() => {
		const onScroll = () => (scrolled = window.scrollY > 24);
		onScroll();
		measure();
		window.addEventListener('scroll', onScroll, { passive: true });
		window.addEventListener('resize', measure);
		return () => {
			window.removeEventListener('scroll', onScroll);
			window.removeEventListener('resize', measure);
		};
	});
</script>

<a
	href="#content"
	class="sr-only z-[60] rounded-full bg-primary px-4 py-2 text-sm text-primary-foreground focus:not-sr-only focus:fixed focus:top-3 focus:left-3"
>
	Skip to content
</a>
<header class="site-header fixed inset-x-0 top-0 z-50" data-scrolled={scrolled ? '' : undefined}>
	<div class="shell">
		<div bind:this={bar} class="bar relative mt-4 flex h-16 items-center justify-between rounded-[40px] p-2 dt:mx-4 dt:h-[71px]">
			<span class="pill-ground pointer-events-none absolute inset-0 rounded-[40px]" aria-hidden="true"></span>
			<a
				href="/"
				data-nav-part
				class="logo relative flex items-center gap-1.5 rounded-full py-1 pr-2 pl-2 text-xl leading-6 font-semibold tracking-normal outline-none focus-visible:ring-2 focus-visible:ring-ring"
			>
				<Mark size={22} />
				<span>Colophon</span>
				<span class="sr-only">, home</span>
			</a>

			<nav aria-label="Main" class="relative hidden dt:block" data-nav-part>
				<ul class="flex items-center gap-2">
					{#each NAV as link (link.href)}
						<li>
							<a
								href={link.href}
								rel={link.external ? 'external' : undefined}
								aria-current={!link.external && current(link.href) ? 'page' : undefined}
								class="nav-link block rounded-full px-3 py-2 text-base leading-6 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
							>
								{link.label}
							</a>
						</li>
					{/each}
				</ul>
			</nav>

			<div class="right relative flex items-center gap-2" data-nav-part>
				<ThemeToggle />
				<a
					href={GITHUB}
					class="hidden size-10 place-items-center rounded-full text-foreground outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring dt:grid"
					aria-label="Colophon on GitHub"
				>
					<BrandIcon name="github" size={20} />
				</a>
				<Pill label="Get started" href={QUICKSTART} external class="hidden dt:inline-flex" />
				<Sheet.Root bind:open={menu}>
					<Sheet.Trigger class="grid size-12 place-items-center rounded-full outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring dt:hidden" aria-label="Menu">
						<MenuIcon class="size-5" aria-hidden="true" />
					</Sheet.Trigger>
					<Sheet.Content side="top" class="mx-2 mt-2 rounded-[32px] border-0 bg-popover p-6 shadow-[var(--shadow-float)]">
						<Sheet.Header class="p-0">
							<Sheet.Title class="flex items-center gap-2 text-xl font-semibold"><Mark size={20} />Colophon</Sheet.Title>
							<Sheet.Description>Pre-release: v0.1 is not out yet.</Sheet.Description>
						</Sheet.Header>
						<nav aria-label="Main, on a small screen" class="mt-4">
							<ul class="grid gap-1">
								<li><a href="/" onclick={() => (menu = false)} class="block rounded-2xl px-3 py-2.5 text-base hover:bg-accent">Home</a></li>
								{#each NAV as link (link.href)}
									<li>
										<a
											href={link.href}
											rel={link.external ? 'external' : undefined}
											onclick={() => (menu = false)}
											aria-current={!link.external && current(link.href) ? 'page' : undefined}
											class="block rounded-2xl px-3 py-2.5 text-base hover:bg-accent aria-[current=page]:bg-accent aria-[current=page]:font-medium"
										>
											{link.label}
										</a>
									</li>
								{/each}
								<li><a href="/open-source/" onclick={() => (menu = false)} class="block rounded-2xl px-3 py-2.5 text-base hover:bg-accent">Open source</a></li>
								<li><a href={GITHUB} class="block rounded-2xl px-3 py-2.5 text-base hover:bg-accent">GitHub</a></li>
							</ul>
						</nav>
						<Pill label="Get started" href={QUICKSTART} external class="mt-4 w-full" />
					</Sheet.Content>
				</Sheet.Root>
			</div>
		</div>
	</div>
</header>

<style>
	/* Below 1,200px the bar is always a pill; on a desktop it appears once the page scrolls. */
	.pill-ground {
		background: var(--nav-glass);
		box-shadow: inset 0 0 0 1px var(--nav-edge);
		-webkit-backdrop-filter: blur(10px);
		backdrop-filter: blur(10px);
	}
	.nav-link {
		transition: background-color 200ms ease;
	}
	.nav-link:hover,
	.nav-link[aria-current='page'] {
		background: var(--tertiary-hover);
	}
	@media (min-width: 75rem) {
		.pill-ground {
			left: var(--nav-inset, 0px);
			right: var(--nav-inset, 0px);
			opacity: 0;
			transform: scaleX(1.06);
			transition:
				opacity 500ms cubic-bezier(0.22, 1, 0.36, 1),
				transform 500ms cubic-bezier(0.22, 1, 0.36, 1);
		}
		.logo,
		.right {
			transition: transform 500ms cubic-bezier(0.22, 1, 0.36, 1);
		}
		[data-scrolled] .pill-ground {
			opacity: 1;
			transform: none;
		}
		[data-scrolled] .logo {
			transform: translateX(calc(var(--nav-inset, 0px) + 16px));
		}
		[data-scrolled] .right {
			transform: translateX(calc(var(--nav-inset, 0px) * -1));
		}
	}
</style>
