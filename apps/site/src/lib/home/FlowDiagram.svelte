<!--
  What happens when someone publishes, as a data flow (D-045, D-046, D-050, D-051): the request
  reaches the API; the release and its delivery jobs commit in one PostgreSQL transaction (the
  outbox); a worker claims each job under a lease and calls out with a stable idempotency key;
  the probe's sighting becomes the receipt. Dots travel each wire in turn on a nine-second loop;
  the resting frame shows the whole path. Horizontal on wide screens, vertical on narrow ones.
-->
<script lang="ts">
	import { inView } from '#lib/motion.ts';

	const NODES = [
		{ name: 'Studio · CLI · agents', lines: ['POST …/publish', 'one API for all'] },
		{ name: 'colophon-server', lines: ['Rust · Axum · Tokio', 'one permission check'] },
		{ name: 'PostgreSQL', lines: ['release 42', 'outbox: 3 jobs', 'one transaction'], authority: true },
		{ name: 'colophon-worker', lines: ['claims under a lease', 'same idempotency key on retry'] },
		{ name: 'Destinations', lines: ['webhook → delivered', 'CDN → purge accepted', 'probe → saw release 42'] }
	];
	const WIRES = ['HTTPS', 'COMMIT', 'claim', 'deliver'];
</script>

<div class="flow" {@attach inView()} aria-hidden="true">
	{#each NODES as node, index (node.name)}
		<div class="node live relative rounded-2xl border bg-card p-4 shadow-soft {node.authority ? 'border-brand/50' : 'border-border/70'}" style="--i: {index}">
			<p class="font-mono text-[0.75rem] font-semibold">{node.name}</p>
			<ul class="mt-2 grid gap-1">
				{#each node.lines as line, row (line)}
					<li class="line relative overflow-hidden rounded-md px-1.5 py-0.5 font-mono text-[0.6875rem] text-muted-foreground">
						<span class="flash live absolute inset-0 bg-brand-soft" style="--i: {index}; --r: {row}"></span><span class="relative">{line}</span>
					</li>
				{/each}
			</ul>
			{#if index === NODES.length - 1}
				<p class="mt-3 inline-flex items-center gap-1.5 rounded-full bg-live-soft px-2.5 py-0.5 text-[0.6875rem] font-medium text-live-ink">✓ Verified at 1 location</p>
			{/if}
		</div>
		{#if index < WIRES.length}
			<div class="wire relative" style="--i: {index}">
				<span class="track"></span>
				<span class="runner live"><span class="dot"></span></span>
				<span class="label font-mono text-[0.625rem] text-muted-foreground">{WIRES[index]}</span>
			</div>
		{/if}
	{/each}
</div>

<style>
	.flow {
		display: grid;
		gap: 0;
	}
	/* Narrow: a column, wires running down. */
	.wire {
		height: 2.75rem;
		width: 100%;
	}
	.track {
		position: absolute;
		left: 50%;
		top: 0;
		bottom: 0;
		border-left: 1.5px dashed var(--border);
	}
	.runner {
		position: absolute;
		inset: 0;
		opacity: 0;
	}
	.dot {
		position: absolute;
		left: 50%;
		top: 0;
		width: 0.625rem;
		height: 0.625rem;
		margin: -0.3125rem 0 0 -0.3125rem;
		border-radius: 999px;
		background: var(--brand);
		box-shadow: 0 0 0 4px var(--brand-soft);
	}
	.label {
		position: absolute;
		left: calc(50% + 0.75rem);
		top: 50%;
		transform: translateY(-50%);
	}
	.flash {
		opacity: 0;
	}
	.node::after {
		content: '';
		position: absolute;
		inset: -1px;
		border-radius: inherit;
		border: 1.5px solid var(--brand);
		opacity: 0;
		pointer-events: none;
	}

	/* Wide: a row, wires running across. */
	@media (min-width: 1024px) {
		.flow {
			grid-template-columns: repeat(4, minmax(0, 1fr) 3.25rem) minmax(0, 1fr);
			align-items: center;
		}
		.wire {
			height: 2rem;
		}
		.track {
			left: 0;
			right: 0;
			top: 50%;
			bottom: auto;
			border-left: 0;
			border-top: 1.5px dashed var(--border);
		}
		.dot {
			left: 0;
			top: 50%;
		}
		.label {
			left: 50%;
			top: -0.25rem;
			transform: translateX(-50%);
		}
	}

	/* The loop: each wire carries a dot in turn; each node lights as it arrives. */
	:global([data-run]) .runner {
		animation: travel-down 9s cubic-bezier(0.45, 0, 0.25, 1) infinite both;
		animation-delay: calc(var(--i) * 1.5s);
	}
	:global([data-run]) .node::after {
		animation: arrive 9s ease-out infinite both;
		animation-delay: calc(var(--i) * 1.5s - 0.3s);
	}
	:global([data-run]) .flash {
		animation: flash 9s ease infinite both;
		animation-delay: calc(var(--i) * 1.5s - 0.2s + var(--r) * 0.35s);
	}
	@media (min-width: 1024px) {
		:global([data-run]) .runner {
			animation-name: travel-across;
		}
	}
	@keyframes travel-down {
		0% { opacity: 0; transform: translateY(0); }
		1% { opacity: 1; transform: translateY(0); }
		13% { opacity: 1; transform: translateY(100%); }
		14%, 100% { opacity: 0; transform: translateY(100%); }
	}
	@keyframes travel-across {
		0% { opacity: 0; transform: translateX(0); }
		1% { opacity: 1; transform: translateX(0); }
		13% { opacity: 1; transform: translateX(100%); }
		14%, 100% { opacity: 0; transform: translateX(100%); }
	}
	@keyframes arrive {
		0% { opacity: 0; }
		3% { opacity: 1; }
		12%, 100% { opacity: 0; }
	}
	@keyframes flash {
		0% { opacity: 0; }
		3% { opacity: 1; }
		16%, 100% { opacity: 0; }
	}
</style>
