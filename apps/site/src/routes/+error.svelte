<script lang="ts">
	import { page } from '$app/state';
	import Pill from '#lib/components/Pill.svelte';

	const missing = $derived(page.status === 404);
</script>

<svelte:head>
	<title>{missing ? 'Page not found' : 'Something went wrong'} · Colophon</title>
</svelte:head>

<section class="hero-ground pt-32 pb-24 tb:pt-[9.5rem] dt:pt-40 dt:pb-32">
	<div class="shell text-center">
		<p class="chip figure">{page.status}</p>
		<h1 class="t-h1 mx-auto mt-4 max-w-[49.5rem]">{missing ? 'No page at this address' : 'Something went wrong'}</h1>
		<p class="t-lead mx-auto mt-4 max-w-[43.75rem] text-pretty">
			{#if missing}
				It may have moved, or it never existed. Unlike a release, this page has no receipt.
			{:else}
				{page.error?.message ?? 'An unexpected error.'}
			{/if}
		</p>
		<div class="mt-10 flex flex-col justify-center gap-2 tb:flex-row">
			<Pill label="Go home" href="/" />
			<Pill label="See what changed" href="/changelog/" variant="tertiary" />
		</div>
	</div>
</section>
