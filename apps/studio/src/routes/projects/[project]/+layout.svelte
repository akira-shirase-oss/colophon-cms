<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import { NavigationRail, TopAppBar, AppearanceMenu } from '@colophon/ui';
	import { createQuery } from '@tanstack/svelte-query';
	import { me } from '#lib/api.ts';
	import mark from '#lib/colophon-mark.svg';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const project = $derived(page.params.project ?? '');
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));

	// Determine active section based on route
	const activeSection = $derived(() => {
		const path = page.url.pathname;
		if (path.includes('/changesets')) return 'Changesets';
		if (path.includes('/releases')) return 'Releases';
		if (path.includes('/members')) return 'Members';
		return 'Content';
	});

	// Navigation items for the project
	const navItems = $derived([
		{ label: 'Content', href: resolve('/projects/[project]', { project }), icon: '📄' },
		{ label: 'Changesets', href: resolve('/projects/[project]/changesets', { project }), icon: '📋' },
		{ label: 'Releases', href: resolve('/projects/[project]/releases', { project }), icon: '🚀' },
		{ label: 'Members', href: resolve('/projects/[project]/members', { project }), icon: '👥' }
		// Settings will be added later when the route exists
	]);
</script>

<NavigationRail items={navItems} activeLabel={activeSection()} />

<div class="main-content">
	<TopAppBar>
		{#snippet title()}
			<a class="home" href={resolve('/')}>
				<img src={mark} alt="" width="22" height="24" class="mark" />
				<span>Colophon Studio</span>
			</a>
			<span class="separator">/</span>
			<span class="project-name">{project}</span>
		{/snippet}
		{#snippet trailing()}
			<AppearanceMenu />
		{/snippet}
	</TopAppBar>

	<main>{@render children()}</main>
</div>

<style>
	.main-content {
		margin-left: 16rem;
		min-height: 100vh;
		display: flex;
		flex-direction: column;
	}

	.home {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		color: inherit;
		text-decoration: none;
	}

	.mark {
		flex: none;
	}

	:global([data-theme='dark']) .mark {
		filter: invert(1);
	}

	@media (prefers-color-scheme: dark) {
		:global(:root:not([data-theme='light'])) .mark {
			filter: invert(1);
		}
	}

	.separator {
		color: var(--md-sys-color-on-surface-variant);
		margin: 0 0.5rem;
	}

	.project-name {
		font: var(--md-sys-typescale-title-medium);
		color: var(--md-sys-color-on-surface);
	}

	main {
		flex: 1;
		padding: 1.5rem 2rem 3rem;
		max-width: 80rem;
	}

	@media (max-width: 768px) {
		.main-content {
			margin-left: 0;
		}
	}
</style>
