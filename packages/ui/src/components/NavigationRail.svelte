<!-- A Material Design 3 navigation rail (left sidebar) for project-level navigation. -->
<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import type { Snippet } from 'svelte';

	type NavItem = {
		label: string;
		href: string;
		icon: string;
	};

	let { items, activeLabel }: { items: NavItem[]; activeLabel?: string } = $props();
</script>

<nav class="navigation-rail" aria-label="Project navigation">
	<ul class="nav-list">
		{#each items as item (item.href)}
			<li>
				<a
					href={item.href}
					class:active={item.label === activeLabel}
					aria-current={item.label === activeLabel ? 'page' : undefined}
				>
					<span class="icon" aria-hidden="true">{item.icon}</span>
					<span class="label">{item.label}</span>
				</a>
			</li>
		{/each}
	</ul>
</nav>

<style>
	.navigation-rail {
		width: 16rem;
		min-height: 100vh;
		background: var(--md-sys-color-surface-container);
		border-right: 1px solid var(--md-sys-color-outline-variant);
		padding: 1rem 0.5rem;
		position: fixed;
		left: 0;
		top: 0;
	}

	.nav-list {
		list-style: none;
		padding: 0;
		margin: 0;
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}

	.nav-list a {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		padding: 0.75rem 1rem;
		border-radius: var(--md-sys-shape-corner-large);
		text-decoration: none;
		color: var(--md-sys-color-on-surface-variant);
		font: var(--md-sys-typescale-label-large);
		transition: background 0.2s, color 0.2s;
	}

	.nav-list a:hover {
		background: var(--md-sys-color-surface-container-high);
		color: var(--md-sys-color-on-surface);
	}

	.nav-list a.active {
		background: var(--md-sys-color-secondary-container);
		color: var(--md-sys-color-on-secondary-container);
		font-weight: 500;
	}

	.icon {
		font-size: 1.5rem;
		flex: none;
		display: flex;
		align-items: center;
		justify-content: center;
	}

	.label {
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	@media (max-width: 768px) {
		.navigation-rail {
			display: none;
		}
	}
</style>
