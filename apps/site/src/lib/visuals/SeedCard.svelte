<!-- The simulator (P1, P2): a run is a seed, and a failing seed replays exactly. The commands are
     the real ones (AGENTS.md, .github/workflows/ci.yml); the output lines describe what they check. -->
<script lang="ts">
	let { class: className = '' }: { class?: string } = $props();
</script>

<div class="overflow-hidden rounded-2xl border border-border bg-card font-mono text-[0.75rem] leading-relaxed shadow-hairline {className}">
	<div class="flex items-center gap-1.5 border-b border-border px-4 py-2.5" aria-hidden="true">
		<span class="size-2 rounded-full bg-border"></span><span class="size-2 rounded-full bg-border"></span><span class="size-2 rounded-full bg-border"></span>
	</div>
	<div class="space-y-1 overflow-x-auto px-4 py-3">
		<p><span class="text-muted-foreground">$</span> colophon-sim --scenario publishing --seeds 1000</p>
		<p class="text-muted-foreground">publishers race, workers crash, the network drops and duplicates</p>
		<p class="text-muted-foreground">rules checked after every step</p>
		<p class="pt-2"><span class="text-muted-foreground">$</span> colophon-sim --seed N --trace</p>
		<p class="text-muted-foreground">a failing seed replays exactly</p>
	</div>
</div>
