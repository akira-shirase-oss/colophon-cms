<script lang="ts">
	import '@colophon/ui/tokens.css';
	import '@colophon/ui/base.css';
	import { AppearanceMenu, TopAppBar } from '@colophon/ui';
	import mark from '#lib/colophon-mark.svg';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="icon" href={mark} />
	<title>Colophon Studio</title>
</svelte:head>

<TopAppBar>
	{#snippet title()}
		<img src={mark} alt="" width="22" height="24" class="mark" />
		<span>Colophon Studio</span>
		<span class="tag">editor · step 0.7</span>
	{/snippet}
	{#snippet trailing()}
		<AppearanceMenu />
	{/snippet}
</TopAppBar>

<main>
	{@render children()}
</main>

<style>
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
	.tag {
		font: var(--md-sys-typescale-label-medium);
		color: var(--md-sys-color-on-surface-variant);
		padding: 0.125rem 0.5rem;
		border-radius: var(--md-sys-shape-corner-small);
		background: var(--md-sys-color-surface-container-highest);
	}
	main {
		max-width: 80rem;
		margin: 0 auto;
		padding: 1.5rem 1rem 3rem;
	}
</style>
