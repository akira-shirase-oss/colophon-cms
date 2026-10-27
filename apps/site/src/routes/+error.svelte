<script lang="ts">
	import { page } from '$app/state';
	import { Button } from '@colophon/ui/ui/button';
	import Mark from '#lib/components/Mark.svelte';

	const missing = $derived(page.status === 404);
</script>

<svelte:head>
	<title>{missing ? 'Page not found' : 'Something went wrong'} · Colophon</title>
</svelte:head>

<section class="shell pt-10">
	<div class="glow grid min-h-[60svh] place-items-center rounded-[2rem] px-4 py-16 sm:py-20">
		<div class="w-full max-w-lg rounded-[2rem] bg-card p-8 text-center shadow-float sm:p-12">
			<span class="mx-auto grid size-14 place-items-center rounded-2xl bg-brand-soft text-brand-ink"><Mark size={26} /></span>
			<p class="figure mt-6 text-sm text-muted-foreground">{page.status}</p>
			<h1 class="display-2 mt-2">{missing ? 'No page at this address' : 'Something went wrong'}</h1>
			<p class="mt-4 text-[0.9375rem] leading-relaxed text-muted-foreground">
				{#if missing}
					It may have moved, or it never existed. Unlike a release, this page has no receipt.
				{:else}
					{page.error?.message ?? 'An unexpected error.'}
				{/if}
			</p>
			<div class="mt-8 flex flex-col justify-center gap-3 sm:flex-row">
				<Button href="/">Go home</Button>
				<Button variant="outline" href="/changelog/">See what changed</Button>
			</div>
		</div>
	</div>
</section>
