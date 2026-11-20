<script lang="ts">
	let { data } = $props();
	const r = $derived(data.recipe);
</script>

<svelte:head><title>{r.title} · Butter &amp; Thyme</title></svelte:head>

<article>
	<h1>{r.title}</h1>
	{#if r.intro}<p>{r.intro}</p>{/if}
	<p>
		{#if r.category}{r.category}{/if}
		{#if r.difficulty} · {r.difficulty}{/if}
		{#if r.prep_minutes != null} · prep {r.prep_minutes} min{/if}
		{#if r.cook_minutes != null} · cook {r.cook_minutes} min{/if}
		{#if r.servings != null} · serves {r.servings}{/if}
	</p>
	{#if r.ingredients?.length}
		<h2>Ingredients</h2>
		<ul>{#each r.ingredients as i}<li>{#if i.amount}{i.amount} {/if}{i.item}</li>{/each}</ul>
	{/if}
	{#if r.steps?.length}
		<h2>Method</h2>
		<ol>{#each r.steps as step}<li>{step}</li>{/each}</ol>
	{/if}
	{#if data.notes}
		<h2>Cook’s notes</h2>
		<!-- Rendered by @colophon/content-renderer from validated rich-text JSON (never stored HTML). -->
		{@html data.notes}
	{/if}
</article>
