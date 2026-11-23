<script lang="ts">
	let { data } = $props();
</script>

<svelte:head><title>Butter &amp; Thyme</title></svelte:head>

{#if data.recipes.length === 0}
	<p>No recipes yet. Seed the demo project first: <code>scripts/seed-demo-sites.sh</code> (apps/sites/README.md).</p>
{:else}
	<h1>Recipes</h1>
	<ul>
		{#each data.recipes as recipe (recipe.slug)}
			<li>
				<a href="/recipes/{recipe.slug}/">{recipe.title}</a>
				{#if recipe.category}<small>{recipe.category}</small>{/if}
				{#if recipe.intro}<p>{recipe.intro}</p>{/if}
			</li>
		{/each}
	</ul>
{/if}
