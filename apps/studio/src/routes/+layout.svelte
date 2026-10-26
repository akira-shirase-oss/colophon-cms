<script lang="ts">
	import '@colophon/ui/tokens.css';
	import '@colophon/ui/base.css';
	import { resolve } from '$app/paths';
	import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
	import { AppearanceMenu, TopAppBar } from '@colophon/ui';
	import { ApiError } from '#lib/api.ts';
	import mark from '#lib/colophon-mark.svg';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	// Server state lives here (D-030). Reads retry only what can succeed on a second try; a
	// refusal or "not found" is an answer, not a hiccup.
	const queryClient = new QueryClient({
		defaultOptions: {
			queries: {
				staleTime: 5_000,
				retry: (count, error) => error instanceof ApiError && error.failure === 'retry' && count < 3
			}
		}
	});
</script>

<svelte:head>
	<link rel="icon" href={mark} />
	<title>Colophon Studio</title>
</svelte:head>

<TopAppBar>
	{#snippet title()}
		<a class="home" href={resolve('/')}>
			<img src={mark} alt="" width="22" height="24" class="mark" />
			<span>Colophon Studio</span>
		</a>
		<span class="tag">step 1.7</span>
	{/snippet}
	{#snippet trailing()}
		<AppearanceMenu />
	{/snippet}
</TopAppBar>

<main>
	<QueryClientProvider client={queryClient}>
		{@render children()}
	</QueryClientProvider>
</main>

<style>
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
