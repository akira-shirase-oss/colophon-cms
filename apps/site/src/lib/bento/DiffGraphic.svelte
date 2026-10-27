<!--
  A publish request approves exact versions (D-044): its hash is computed from them, and a review
  counts only while it keeps that hash. Saving version 8 afterwards makes the approval stale: acting
  on it is refused with 409 and details.reason "stale_changeset". A ten-second loop.
-->
<div class="flex h-full items-center justify-center p-5">
	<div class="w-full max-w-md rounded-2xl border border-border bg-card p-4 shadow-soft">
		<div class="flex items-center justify-between gap-3">
			<p class="text-[0.8125rem] font-semibold">Spring menu <span class="font-normal text-muted-foreground">version 6 → 7</span></p>
			<span class="stack text-[0.6875rem] font-medium">
				<span class="ok live rounded-full bg-live-soft px-2.5 py-0.5 text-live-ink">Approved · 4be1…07c2</span>
				<span class="stale live rounded-full bg-attention-soft px-2.5 py-0.5 font-mono text-attention-ink">409 stale_changeset</span>
			</span>
		</div>
		<div class="mt-3 overflow-hidden rounded-lg border border-border font-mono text-[0.75rem] leading-6">
			<p class="bg-attention-soft/60 px-3 text-attention-ink">− "soup": "leek and potato"</p>
			<p class="bg-live-soft/70 px-3 text-live-ink">+ "soup": "pea and mint"</p>
		</div>
		<div class="stack mt-3 text-xs">
			<p class="rest live text-muted-foreground">The approval is for this exact version and hash.</p>
			<p class="edit live text-pending-ink">version 8 saved after the approval: approve again</p>
		</div>
	</div>
</div>

<style>
	.stack {
		display: grid;
		justify-items: end;
	}
	.stack > * {
		grid-area: 1 / 1;
	}
	div.stack {
		justify-items: start;
	}
	.stale,
	.edit {
		opacity: 0;
	}
	:global([data-run]) .ok,
	:global([data-run]) .rest {
		animation: before 10s ease infinite both;
	}
	:global([data-run]) .stale,
	:global([data-run]) .edit {
		animation: after 10s ease infinite both;
	}
	@keyframes before {
		0%, 44% { opacity: 1; transform: none; }
		48%, 88% { opacity: 0; transform: translateY(-3px); }
		93%, 100% { opacity: 1; transform: none; }
	}
	@keyframes after {
		0%, 46% { opacity: 0; transform: translateY(3px); }
		50%, 86% { opacity: 1; transform: none; }
		90%, 100% { opacity: 0; }
	}
</style>
