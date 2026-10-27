<!--
  Crash-proof saving (F01, F02): the server is killed with kill -9 while version 7 is being saved,
  restarts, and the editor's retry with the same idempotency key gets the same answer, stored
  once; versions 5 and 6 never moved. A ten-second loop; the resting frame is the end of it.
-->
<div class="flex h-full items-center justify-center px-4 pt-7 pb-4 sm:p-5 sm:pt-7">
	<div class="relative w-full max-w-md">
		<div class="box live rounded-2xl border border-border bg-card p-4 shadow-soft">
			<div class="flex items-center justify-between gap-3">
				<span class="font-mono text-[0.8125rem] font-medium">colophon-server</span>
				<span class="stack text-[0.6875rem] font-medium">
					<span class="pill run live" data-tone="live"><i></i>running</span>
					<span class="pill killed live" data-tone="attention"><i></i>killed</span>
					<span class="pill restarting live" data-tone="pending"><i></i>restarting</span>
				</span>
			</div>
			<p class="mt-3 text-[0.6875rem] text-muted-foreground">Saves of “Spring menu”</p>
			<ul class="mt-1.5 grid min-w-0 gap-1.5 text-[0.8125rem]">
				{#each [5, 6] as version (version)}
					<li class="flex items-center justify-between gap-3 rounded-lg bg-muted/70 px-3 py-1.5">
						<span>version {version}</span><span class="text-live-ink">saved ✓</span>
					</li>
				{/each}
				<li class="flex items-center justify-between gap-3 rounded-lg bg-muted/70 px-3 py-1.5">
					<span class="min-w-0 truncate">version 7</span>
					<span class="stack shrink-0 text-right whitespace-nowrap">
						<span class="saving live text-pending-ink">saving…</span>
						<span class="lost live text-attention-ink">no answer</span>
						<span class="once live text-live-ink">same key → saved once ✓</span>
					</span>
				</li>
			</ul>
		</div>
		<span class="stamp live absolute -top-3 right-4 rounded-full bg-attention px-3 py-1 font-mono text-xs font-semibold text-white shadow-press">kill -9</span>
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
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 0.375rem;
		border-radius: 999px;
		padding: 0.125rem 0.625rem;
	}
	.pill i {
		width: 0.375rem;
		height: 0.375rem;
		border-radius: 999px;
	}
	.pill[data-tone='live'] {
		background: var(--live-soft);
		color: var(--live-ink);
	}
	.pill[data-tone='live'] i {
		background: var(--live);
	}
	.pill[data-tone='attention'] {
		background: var(--attention-soft);
		color: var(--attention-ink);
	}
	.pill[data-tone='attention'] i {
		background: var(--attention);
	}
	.pill[data-tone='pending'] {
		background: var(--pending-soft);
		color: var(--pending-ink);
	}
	.pill[data-tone='pending'] i {
		background: var(--pending);
	}

	/* The resting frame: running, version 7 saved once, no stamp. */
	.killed,
	.restarting,
	.saving,
	.lost,
	.stamp {
		opacity: 0;
	}

	/* The loop: running and saving until 18%, killed until 32%, restarting until 46%, then
	 * running again with version 7 saved once. */
	:global([data-run]) .run {
		animation: run 10s linear infinite both;
	}
	:global([data-run]) .saving {
		animation: before-kill 10s linear infinite both;
	}
	:global([data-run]) .killed {
		animation: killed 10s linear infinite both;
	}
	:global([data-run]) .lost {
		animation: down 10s linear infinite both;
	}
	:global([data-run]) .restarting {
		animation: restarting 10s linear infinite both;
	}
	:global([data-run]) .once {
		animation: after-restart 10s linear infinite both;
	}
	:global([data-run]) .stamp {
		animation: stamp 10s cubic-bezier(0.3, 1.4, 0.5, 1) infinite both;
	}
	:global([data-run]) .box {
		animation: jolt 10s linear infinite both;
	}
	@keyframes run {
		0%, 17.9% { opacity: 1; }
		18%, 45.9% { opacity: 0; }
		46%, 100% { opacity: 1; }
	}
	@keyframes before-kill {
		0%, 17.9% { opacity: 1; }
		18%, 100% { opacity: 0; }
	}
	@keyframes killed {
		0%, 17.9% { opacity: 0; }
		18%, 31.9% { opacity: 1; }
		32%, 100% { opacity: 0; }
	}
	@keyframes down {
		0%, 17.9% { opacity: 0; }
		18%, 45.9% { opacity: 1; }
		46%, 100% { opacity: 0; }
	}
	@keyframes restarting {
		0%, 31.9% { opacity: 0; }
		32%, 45.9% { opacity: 1; }
		46%, 100% { opacity: 0; }
	}
	@keyframes after-restart {
		0%, 45.9% { opacity: 0; transform: translateY(3px); }
		47%, 100% { opacity: 1; transform: translateY(0); }
	}
	@keyframes stamp {
		0%,
		16% {
			opacity: 0;
			transform: scale(0.6) rotate(-8deg);
		}
		19%,
		40% {
			opacity: 1;
			transform: scale(1) rotate(-4deg);
		}
		46%,
		100% {
			opacity: 0;
			transform: scale(0.9) rotate(-4deg);
		}
	}
	@keyframes jolt {
		0%,
		18%,
		21%,
		100% {
			transform: translateX(0);
		}
		18.6% {
			transform: translateX(-3px);
		}
		19.4% {
			transform: translateX(3px);
		}
		20.2% {
			transform: translateX(-1px);
		}
	}
</style>
