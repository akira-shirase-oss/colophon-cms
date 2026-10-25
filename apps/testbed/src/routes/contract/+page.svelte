<script lang="ts">
	import { checks, type Outcome } from '#lib/contract.ts';

	let outcomes = $state<Record<string, Outcome | 'running'>>({});
	let running = $state(false);

	let finished = $derived(Object.values(outcomes).filter((value) => value !== 'running') as Outcome[]);
	let passed = $derived(finished.filter((value) => value.pass).length);

	async function runAll() {
		running = true;
		outcomes = {};
		for (const check of checks) {
			outcomes[check.id] = 'running';
			try {
				outcomes[check.id] = await check.run();
			} catch (error) {
				outcomes[check.id] = { pass: false, detail: `the request failed: ${String(error)}`, reply: null };
			}
		}
		running = false;
	}
</script>

<h1>API contract</h1>
<p class="lede">
	The step 0.3 contract, checked from a browser exactly as a client sees it: health answers, one error envelope for every
	error (including those from middleware), and request ids. Playwright runs this same list in CI.
</p>

<p>
	<button onclick={runAll} disabled={running}>{running ? 'Running…' : 'Run all checks'}</button>
	{#if finished.length === checks.length && !running}
		<span class="summary" data-testid="summary">{passed} of {checks.length} passed</span>
	{/if}
</p>

<ol class="checks">
	{#each checks as check (check.id)}
		{@const result = outcomes[check.id]}
		<li
			data-testid="check-{check.id}"
			data-result={result === undefined ? 'pending' : result === 'running' ? 'running' : result.pass ? 'pass' : 'fail'}
		>
			<div class="row">
				<strong>{check.title}</strong>
				{#if result === 'running'}
					<span class="pill">running</span>
				{:else if result}
					<span class="pill {result.pass ? 'ok' : 'bad'}">{result.pass ? 'pass' : 'fail'}</span>
				{/if}
			</div>
			<p class="proves">{check.proves}</p>
			{#if result && result !== 'running'}
				<p class="detail">{result.detail}</p>
				{#if result.reply && result.reply.text}
					<details>
						<summary>Response</summary>
						<pre>{result.reply.text}</pre>
					</details>
				{/if}
			{/if}
		</li>
	{/each}
</ol>

<style>
	.lede,
	.proves {
		color: var(--muted);
	}
	.lede {
		max-width: 46rem;
	}
	button {
		font: inherit;
		padding: 0.5rem 1rem;
		border-radius: 8px;
		border: 1px solid var(--line);
		background: var(--fg);
		color: var(--bg);
		cursor: pointer;
	}
	button:disabled {
		opacity: 0.6;
		cursor: progress;
	}
	.summary {
		margin-left: 0.8rem;
		font-weight: 600;
	}
	.checks {
		padding-left: 1.2rem;
		display: grid;
		gap: 0.8rem;
	}
	.checks li {
		padding: 0.8rem;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--card);
	}
	.row {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
	}
	.proves,
	.detail {
		margin: 0.35rem 0 0;
	}
	.pill {
		border-radius: 999px;
		padding: 0.1rem 0.55rem;
		font-weight: 600;
		font-size: 0.8rem;
		background: var(--line);
	}
	.ok {
		color: var(--ok);
		background: var(--ok-bg);
	}
	.bad {
		color: var(--bad);
		background: var(--bad-bg);
	}
	pre {
		white-space: pre-wrap;
		word-break: break-all;
	}
</style>
