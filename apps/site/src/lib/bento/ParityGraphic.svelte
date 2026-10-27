<!--
  Parity with Strapi and Sanity (D-057): the backlog's rows by status, and the planned ones by the
  milestone that builds them, counted from the comparison's data. Bars grow once in view.
-->
<script lang="ts">
	let {
		done,
		building,
		planned,
		milestones
	}: { done: number; building: number; planned: number; milestones: { id: string; count: number }[] } = $props();
	const total = $derived(done + building + planned);
	const top = $derived(Math.max(...milestones.map((milestone) => milestone.count)));
</script>

<div class="flex h-full flex-col justify-center gap-4 p-5">
	<div>
		<div class="flex h-3 overflow-hidden rounded-full bg-muted">
			<span class="seg reveal bg-live" style="width: {(done / total) * 100}%; --i: 0"></span>
			<span class="seg reveal bg-pending" style="width: {(building / total) * 100}%; --i: 1"></span>
			<span class="seg reveal bg-draft/50" style="width: {(planned / total) * 100}%; --i: 2"></span>
		</div>
		<p class="mt-2 flex flex-wrap gap-x-4 gap-y-1 text-xs">
			<span><span class="figure font-semibold">{done}</span> <span class="text-muted-foreground">done</span></span>
			<span><span class="figure font-semibold">{building}</span> <span class="text-muted-foreground">building</span></span>
			<span><span class="figure font-semibold">{planned}</span> <span class="text-muted-foreground">planned</span></span>
			<span class="text-muted-foreground">of {total} backlog rows</span>
		</p>
	</div>
	<div class="flex h-20 items-end gap-2">
		{#each milestones as milestone, index (milestone.id)}
			<div class="flex flex-1 flex-col items-center gap-1">
				<span class="figure text-[0.6875rem] text-muted-foreground">{milestone.count}</span>
				<span class="col reveal w-full max-w-9 origin-bottom rounded-md bg-chart-2" style="height: {(milestone.count / top) * 3}rem; --i: {index}"></span>
				<span class="font-mono text-[0.6875rem] font-medium">{milestone.id}</span>
			</div>
		{/each}
	</div>
</div>

<style>
	.seg {
		transform-origin: left;
	}
	:global([data-run]) .seg {
		animation: grow-x 700ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
		animation-delay: calc(var(--i) * 250ms);
	}
	:global([data-run]) .col {
		animation: grow-y 700ms cubic-bezier(0.2, 0.8, 0.2, 1) both;
		animation-delay: calc(400ms + var(--i) * 80ms);
	}
	@keyframes grow-x {
		from {
			transform: scaleX(0);
		}
	}
	@keyframes grow-y {
		from {
			transform: scaleY(0);
		}
	}
</style>
