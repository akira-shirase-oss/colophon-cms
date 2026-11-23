<script lang="ts">
	let { data, children } = $props();
</script>

<svelte:head>
	<!-- The release this page was built from: a Colophon site check reads it to verify a release reached the site. -->
	{#if data.generation !== null}<meta name="colophon-generation" content={String(data.generation)} />{/if}
</svelte:head>

<header><a href="/">Butter &amp; Thyme</a> <small>a SvelteKit demo of Colophon</small></header>
<main>{@render children()}</main>
<footer>
	{#if data.generation !== null}Built from release {data.generation}.{/if}
</footer>

<style>
	:global(body) { font-family: system-ui, sans-serif; max-width: 42rem; margin: 2rem auto; padding: 0 1rem; line-height: 1.5; }
	header { display: flex; gap: 0.75rem; align-items: baseline; margin-bottom: 2rem; }
	footer { margin-top: 3rem; color: #555; font-size: 0.875rem; }
</style>
