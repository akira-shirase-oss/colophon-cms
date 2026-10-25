<script lang="ts">
	import { call, isReadinessReport, type ReadinessReport } from '#lib/api.ts';

	interface Transition {
		at: string;
		from: string;
		to: string;
	}

	const POLL_MS = 1000;

	let report = $state<ReadinessReport | null>(null);
	let httpStatus = $state<number | null>(null);
	let requestId = $state<string | null>(null);
	let latency = $state<number | null>(null);
	let unreachable = $state(false);
	let transitions = $state<Transition[]>([]);

	let label = $derived(unreachable ? 'unreachable' : (report?.status ?? 'loading'));

	async function poll() {
		let next: string;
		try {
			const reply = await call('GET', '/readyz');
			httpStatus = reply.status;
			requestId = reply.requestId;
			latency = reply.ms;
			if (isReadinessReport(reply.body)) {
				report = reply.body;
				unreachable = false;
				next = reply.body.status;
			} else {
				unreachable = true;
				next = 'unreachable';
			}
		} catch {
			unreachable = true;
			httpStatus = null;
			next = 'unreachable';
		}
		const previous = transitions[0]?.to;
		if (previous !== next) {
			transitions = [{ at: new Date().toLocaleTimeString(), from: previous ?? '—', to: next }, ...transitions].slice(0, 20);
		}
	}

	$effect(() => {
		void poll();
		const timer = setInterval(() => void poll(), POLL_MS);
		return () => clearInterval(timer);
	});

	function tone(state: string): string {
		if (state === 'ready' || state === 'up' || state === 'not_configured') return 'ok';
		if (state === 'degraded' || state === 'starting' || state === 'draining' || state === 'unknown') return 'warn';
		return 'bad';
	}
</script>

<h1>Health</h1>
<p class="lede">
	What a load balancer sees: <code>/readyz</code>, polled every second through the real HTTP API. Stop a service
	(<code>docker compose -f deploy/compose.yaml stop valkey</code>) and watch the state change.
</p>

<section class="status" data-testid="status" data-status={label}>
	<span class="pill {tone(label)}">{label}</span>
	<dl>
		<div><dt>HTTP</dt><dd data-testid="http-status">{httpStatus ?? '—'}</dd></div>
		<div><dt>Role</dt><dd>{report?.role ?? '—'}</dd></div>
		<div><dt>Latency</dt><dd>{latency === null ? '—' : `${latency.toFixed(1)} ms`}</dd></div>
		<div><dt>Request id</dt><dd><code>{requestId ?? '—'}</code></dd></div>
	</dl>
</section>

<h2>Dependencies</h2>
<ul class="checks">
	{#each Object.entries(report?.checks ?? {}) as [name, state] (name)}
		<li data-testid="check-{name}" data-state={state}>
			<span>{name}</span>
			<span class="pill {tone(state)}">{state}</span>
		</li>
	{:else}
		<li>No report yet.</li>
	{/each}
</ul>

<h2>State changes</h2>
<ol class="log" data-testid="transitions">
	{#each transitions as change (change.at + change.to)}
		<li><time>{change.at}</time> {change.from} → <strong>{change.to}</strong></li>
	{/each}
</ol>

<style>
	.lede {
		color: var(--muted);
		max-width: 46rem;
	}
	.status {
		display: flex;
		align-items: center;
		gap: 1.25rem;
		flex-wrap: wrap;
		padding: 1rem;
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--card);
	}
	.status .pill {
		font-size: 1.2rem;
		padding: 0.4rem 0.9rem;
	}
	dl {
		display: flex;
		gap: 1.5rem;
		margin: 0;
		flex-wrap: wrap;
	}
	dt {
		color: var(--muted);
		font-size: 0.8rem;
	}
	dd {
		margin: 0.15rem 0 0;
	}
	.pill {
		display: inline-block;
		border-radius: 999px;
		padding: 0.15rem 0.6rem;
		font-weight: 600;
		font-size: 0.85rem;
	}
	.ok {
		color: var(--ok);
		background: var(--ok-bg);
	}
	.warn {
		color: var(--warn);
		background: var(--warn-bg);
	}
	.bad {
		color: var(--bad);
		background: var(--bad-bg);
	}
	.checks {
		list-style: none;
		padding: 0;
		display: grid;
		gap: 0.5rem;
		max-width: 28rem;
	}
	.checks li {
		display: flex;
		justify-content: space-between;
		padding: 0.6rem 0.8rem;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--card);
	}
	.log {
		padding-left: 1.2rem;
		color: var(--muted);
	}
	.log strong {
		color: var(--fg);
	}
	time {
		font-variant-numeric: tabular-nums;
	}
</style>
