<!--
  A content type change checked against stored content (D-055, step 3.6, done): making a field
  required while stored entries leave it empty is refused with 422, details.reason
  "needs_migration", and the first entries in the way. The request and field are illustrative;
  the status, code and reason are the server's (crates/colophon-api/src/error.rs).
-->
<div class="flex h-full flex-col justify-center p-5">
	<div class="overflow-hidden rounded-2xl border border-border bg-card font-mono text-[0.6875rem] leading-relaxed shadow-soft">
		<p class="border-b border-border px-3 py-2 break-all"><span class="text-muted-foreground">PUT</span> …/content-types/articles <span class="text-muted-foreground"># a field made required</span></p>
		<div class="grid gap-0.5 px-3 py-2.5">
			<p class="ln live font-semibold" style="--i: 0">HTTP/1.1 422 Unprocessable Entity</p>
			<p class="ln live" style="--i: 1">"code": "validation_failed",</p>
			<p class="ln live" style="--i: 2">"details": {'{'}"reason": <span class="font-semibold text-attention-ink">"needs_migration"</span>,</p>
			<p class="ln live pl-3" style="--i: 3">"entries": [{'{'}"title": "Spring menu", …{'}'}, …],</p>
			<p class="ln live pl-3" style="--i: 4">"total": 3{'}'}</p>
		</div>
	</div>
</div>

<style>
	:global([data-run]) .ln {
		animation: in 7s ease infinite both;
		animation-delay: calc(var(--i) * 0.18s);
	}
	@keyframes in {
		0% { opacity: 0; transform: translateY(3px); }
		4%, 90% { opacity: 1; transform: none; }
		96%, 100% { opacity: 0; }
	}
</style>
