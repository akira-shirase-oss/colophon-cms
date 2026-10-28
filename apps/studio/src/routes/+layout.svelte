<script lang="ts">
	import '../app.css';
	import { page } from '$app/state';
	import { QueryClient, QueryClientProvider } from '@tanstack/svelte-query';
	import { ModeWatcher } from 'mode-watcher';
	import { Toaster } from '@colophon/ui/ui/sonner';
	import * as Tooltip from '@colophon/ui/ui/tooltip';
	import { ApiError } from '#lib/api.ts';
	import mark from '#lib/colophon-mark.svg';
	import Frame from '#lib/shell/Frame.svelte';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	// Server state lives here (D-030). Reads retry only what can succeed on a second try; a
	// refusal or "not found" is an answer, not a hiccup.
	const queryClient = new QueryClient({
		defaultOptions: {
			queries: {
				staleTime: 5_000,
				// A read turned away for the rate limit already waited and went once more (D-076).
				retry: (count, error) => error instanceof ApiError && error.failure === 'retry' && error.reason !== 'rate_limited' && count < 3
			}
		}
	});
	// A project's pages draw their own shell, with its sidebar; every other page gets the plain frame.
	const inProject = $derived(page.route.id?.startsWith('/projects/[project]') ?? false);
</script>

<svelte:head>
	<link rel="icon" href={mark} />
	<title>Colophon Studio</title>
</svelte:head>

<ModeWatcher defaultMode="system" />
<QueryClientProvider client={queryClient}>
	<Tooltip.Provider delayDuration={300}>
		{#if inProject}
			{@render children()}
		{:else}
			<Frame>{@render children()}</Frame>
		{/if}
	</Tooltip.Provider>
</QueryClientProvider>
<Toaster position="bottom-right" />
