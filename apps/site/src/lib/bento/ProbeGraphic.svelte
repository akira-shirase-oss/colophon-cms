<!--
  Delivery receipts (D-051): after release 42, a probe fetches the live page and looks for the
  release's marker, <meta name="colophon-generation" content="42">; seeing it, the release is
  "Verified at 1 location". An eight-second loop; the resting frame is the verified one.
-->
<div class="flex h-full flex-col justify-center gap-4 p-5">
	<div class="flex items-center gap-2">
		<div class="node w-24 shrink-0 rounded-xl border border-border bg-card px-3 py-2 shadow-soft">
			<p class="font-mono text-[0.6875rem] text-muted-foreground">colophon</p>
			<p class="text-xs font-semibold">release 42</p>
		</div>
		<div class="relative h-6 flex-1">
			<span class="absolute top-1/2 right-0 left-0 border-t border-dashed border-border"></span>
			<span class="runner out live absolute inset-0"><span class="absolute top-1/2 left-0 size-2.5 -translate-x-1/2 -translate-y-1/2 rounded-full bg-brand shadow-[0_0_0_4px_var(--brand-soft)]"></span></span>
			<span class="runner back live absolute inset-0">
				<span class="absolute top-1/2 left-0 -translate-x-1/2 -translate-y-1/2 rounded-full bg-live px-1.5 font-mono text-[0.625rem] leading-4 font-semibold text-white">42</span>
			</span>
		</div>
		<div class="site live relative w-24 shrink-0 rounded-xl border border-border bg-card px-3 py-2 shadow-soft">
			<p class="font-mono text-[0.6875rem] text-muted-foreground">GET /</p>
			<p class="text-xs font-semibold">your site</p>
		</div>
	</div>
	<code class="meta relative block overflow-hidden rounded-lg border border-border bg-card px-3 py-2 font-mono text-[0.6875rem] leading-relaxed">
		<span class="mark live absolute inset-0 bg-brand-soft"></span>
		<span class="relative">&lt;meta name="colophon-generation" <span class="font-semibold text-brand-ink">content="42"</span>&gt;</span>
	</code>
	<span class="verdict live mx-auto inline-flex items-center gap-1.5 rounded-full bg-live-soft px-3 py-1 text-xs font-medium text-live-ink">✓ Verified at 1 location</span>
</div>

<style>
	/* The resting frame: no dots in flight, the marker found, the verdict shown. */
	.runner {
		opacity: 0;
	}
	.site::after {
		content: '';
		position: absolute;
		inset: -4px;
		border-radius: 0.9rem;
		border: 2px solid var(--brand);
		opacity: 0;
	}

	:global([data-run]) .out {
		animation: out 8s cubic-bezier(0.45, 0, 0.25, 1) infinite both;
	}
	:global([data-run]) .back {
		animation: back 8s cubic-bezier(0.45, 0, 0.25, 1) infinite both;
	}
	:global([data-run]) .site::after {
		animation: ping 8s ease-out infinite both;
	}
	:global([data-run]) .mark {
		animation: mark 8s ease infinite both;
	}
	:global([data-run]) .verdict {
		animation: verdict 8s cubic-bezier(0.3, 1.4, 0.5, 1) infinite both;
	}
	@keyframes out {
		0%, 4% { opacity: 0; transform: translateX(0); }
		6% { opacity: 1; transform: translateX(0); }
		34% { opacity: 1; transform: translateX(100%); }
		36%, 100% { opacity: 0; transform: translateX(100%); }
	}
	@keyframes ping {
		0%, 34% { opacity: 0; transform: scale(0.96); }
		37% { opacity: 1; transform: scale(1); }
		48%, 100% { opacity: 0; transform: scale(1.08); }
	}
	@keyframes mark {
		0%, 36% { opacity: 0; }
		42%, 94% { opacity: 1; }
		100% { opacity: 0; }
	}
	@keyframes back {
		0%, 44% { opacity: 0; transform: translateX(100%); }
		46% { opacity: 1; transform: translateX(100%); }
		72% { opacity: 1; transform: translateX(0); }
		74%, 100% { opacity: 0; transform: translateX(0); }
	}
	@keyframes verdict {
		0%, 73% { opacity: 0; transform: scale(0.85); }
		77%, 94% { opacity: 1; transform: scale(1); }
		100% { opacity: 0; transform: scale(1); }
	}
</style>
