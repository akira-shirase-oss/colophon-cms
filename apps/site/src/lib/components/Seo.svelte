<!-- A page's title, description and, when it has one elsewhere, its canonical address. -->
<script lang="ts">
	let { title, description, canonical = '' }: { title: string; description: string; canonical?: string } = $props();
</script>

<svelte:head>
	<title>{title}</title>
	<meta name="description" content={description} />
	<meta property="og:title" content={title} />
	<meta property="og:description" content={description} />
	<meta property="og:type" content="website" />
	{#if canonical}<link rel="canonical" href={canonical} />{/if}
</svelte:head>
