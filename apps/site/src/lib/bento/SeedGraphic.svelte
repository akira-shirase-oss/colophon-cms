<!--
  The simulator's determinism (D-039): the same seed runs the same schedule, so two runs end on
  the same trace hash, and a failing seed replays exactly. The command is the real one (AGENTS.md);
  the seed and the shortened hashes are illustrative. A twelve-second loop.
-->
<div class="flex h-full flex-col justify-center p-5">
	<div class="overflow-hidden rounded-2xl border border-border bg-card font-mono text-[0.75rem] leading-relaxed shadow-soft">
		<div class="flex items-center gap-1.5 border-b border-border px-3 py-2">
			<i class="size-2 rounded-full bg-border"></i><i class="size-2 rounded-full bg-border"></i><i class="size-2 rounded-full bg-border"></i>
		</div>
		<div class="grid gap-2 px-3.5 py-3">
			<p class="line l0 live break-all"><span class="text-muted-foreground">$</span> cargo run -p colophon-sim -- --seed 1207 --trace</p>
			{#each [1, 2] as run (run)}
				<div class="line l{run} live grid grid-cols-[3.5rem_minmax(0,1fr)_auto] items-center gap-2">
					<span class="text-muted-foreground">run {run}</span>
					<span class="track h-1.5 overflow-hidden rounded-full bg-muted"><span class="fill f{run} live block h-full origin-left rounded-full bg-chart-2"></span></span>
					<span class="hash h{run} live">a3f9…c21e</span>
				</div>
			{/each}
			<p class="verdict live text-live-ink">✓ same seed, same trace: it replays exactly</p>
		</div>
	</div>
</div>

<style>
	.hash {
		color: var(--brand-ink);
		font-weight: 600;
	}
	/* The loop (12 s): the command, run 1 fills and prints its hash, run 2 the same, then the
	 * verdict; everything rests, and fades out together before the next pass. */
	:global([data-run]) .l0 { animation: l0 12s ease infinite both; }
	:global([data-run]) .l1 { animation: l1 12s ease infinite both; }
	:global([data-run]) .l2 { animation: l2 12s ease infinite both; }
	:global([data-run]) .f1 { animation: f1 12s cubic-bezier(0.4, 0, 0.2, 1) infinite both; }
	:global([data-run]) .f2 { animation: f2 12s cubic-bezier(0.4, 0, 0.2, 1) infinite both; }
	:global([data-run]) .h1 { animation: h1 12s ease infinite both; }
	:global([data-run]) .h2 { animation: h2 12s ease infinite both; }
	:global([data-run]) .verdict { animation: verdict 12s ease infinite both; }
	@keyframes l0 { 0% { opacity: 0; transform: translateY(4px); } 3%, 92% { opacity: 1; transform: none; } 97%, 100% { opacity: 0; } }
	@keyframes l1 { 0%, 5% { opacity: 0; transform: translateY(4px); } 8%, 92% { opacity: 1; transform: none; } 97%, 100% { opacity: 0; } }
	@keyframes l2 { 0%, 24% { opacity: 0; transform: translateY(4px); } 27%, 92% { opacity: 1; transform: none; } 97%, 100% { opacity: 0; } }
	@keyframes f1 { 0%, 8% { transform: scaleX(0); } 20%, 100% { transform: scaleX(1); } }
	@keyframes f2 { 0%, 27% { transform: scaleX(0); } 39%, 100% { transform: scaleX(1); } }
	@keyframes h1 { 0%, 20% { opacity: 0; } 22%, 100% { opacity: 1; } }
	@keyframes h2 { 0%, 39% { opacity: 0; } 41%, 100% { opacity: 1; } }
	@keyframes verdict { 0%, 45% { opacity: 0; transform: translateY(4px); } 48%, 92% { opacity: 1; transform: none; } 97%, 100% { opacity: 0; } }
</style>
