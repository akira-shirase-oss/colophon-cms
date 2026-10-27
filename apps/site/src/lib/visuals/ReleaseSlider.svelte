<!-- Time travel (P4): the site as it was at any release. Planned, so the picture is static and says so. -->
<script lang="ts">
	let { class: className = '' }: { class?: string } = $props();
	const RELEASES = [38, 39, 40, 41, 42];
	const AT = 40;
</script>

<div class="rounded-2xl border border-border bg-card p-4 shadow-hairline {className}" aria-hidden="true">
	<div class="flex items-center justify-between text-xs">
		<span class="font-medium">Viewing release {AT}</span>
		<span class="text-muted-foreground">latest: 42</span>
	</div>
	<div class="relative mt-4 h-8">
		<span class="absolute top-1/2 right-1 left-1 h-1 -translate-y-1/2 rounded-full bg-muted"></span>
		<span class="absolute top-1/2 left-1 h-1 w-1/2 -translate-y-1/2 rounded-full bg-brand"></span>
		{#each RELEASES as release, index (release)}
			<span
				class="absolute top-1/2 grid -translate-x-1/2 -translate-y-1/2 place-items-center"
				style="left: calc(0.25rem + {index / (RELEASES.length - 1)} * (100% - 0.5rem))"
			>
				{#if release === AT}
					<span class="size-5 rounded-full border-2 border-brand bg-card shadow-soft"></span>
				{:else}
					<span class="size-2 rounded-full {release < AT ? 'bg-brand' : 'bg-border'}"></span>
				{/if}
			</span>
		{/each}
	</div>
	<div class="figure mt-1 flex justify-between text-[0.6875rem] text-muted-foreground">
		{#each RELEASES as release (release)}<span>{release}</span>{/each}
	</div>
	<div class="mt-3 flex gap-2">
		<span class="rounded-full border border-border px-3 py-1 text-xs">Open this release</span>
		<span class="rounded-full bg-primary px-3 py-1 text-xs text-primary-foreground">Roll back as release 43</span>
	</div>
</div>
