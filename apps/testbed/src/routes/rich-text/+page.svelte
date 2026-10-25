<script lang="ts">
	import { fixtures } from '#lib/fixtures.ts';
	import { check, validate, type Verdict } from '#lib/richText.ts';
	import type { Reply } from '#lib/api.ts';

	let verdicts = $state<Record<string, Verdict | 'running'>>({});
	let running = $state(false);
	let draft = $state(JSON.stringify(fixtures.find((fixture) => fixture.id === 'valid/12-blog-post')?.value ?? {}, null, 2));
	let tried = $state<Reply | null>(null);

	let finished = $derived(Object.values(verdicts).filter((verdict) => verdict !== 'running') as Verdict[]);
	let passed = $derived(finished.filter((verdict) => verdict.pass).length);

	async function runAll() {
		running = true;
		verdicts = {};
		for (const fixture of fixtures) {
			verdicts[fixture.id] = 'running';
			verdicts[fixture.id] = await check(fixture);
		}
		running = false;
	}

	async function tryDraft() {
		tried = await validate(draft);
	}
</script>

<h1>Rich text</h1>
<p class="lede">
	Every shared fixture sent to <code>POST /api/v1/rich-text/validate</code>, as a client would. Valid documents must come back in
	exactly the canonical form the editor produces; invalid ones must be refused with exactly the code and JSON Pointer the fixture
	names.
</p>

<p>
	<button onclick={runAll} disabled={running}>{running ? 'Running…' : `Check all ${fixtures.length} fixtures`}</button>
	{#if finished.length === fixtures.length && !running}
		<span class="summary" data-testid="summary">{passed} of {fixtures.length} agree</span>
	{/if}
</p>

<div class="table">
	<table>
		<thead><tr><th>Fixture</th><th>What it is</th><th>Server's verdict</th></tr></thead>
		<tbody>
			{#each fixtures as fixture (fixture.id)}
				{@const verdict = verdicts[fixture.id]}
				<tr data-testid="fixture-{fixture.id}" data-result={verdict === undefined ? 'pending' : verdict === 'running' ? 'running' : verdict.pass ? 'pass' : 'fail'}>
					<td><code>{fixture.id}</code></td>
					<td>{fixture.description}</td>
					<td>
						{#if verdict === 'running'}…{:else if verdict}<span class="pill {verdict.pass ? 'ok' : 'bad'}">{verdict.pass ? 'agrees' : 'differs'}</span> {verdict.detail}{/if}
					</td>
				</tr>
			{/each}
		</tbody>
	</table>
</div>

<h2>Try a document</h2>
<textarea bind:value={draft} rows="12" spellcheck="false" aria-label="Rich-text document"></textarea>
<p><button onclick={tryDraft}>Validate</button></p>
{#if tried}
	<p data-testid="try-status">HTTP {tried.status} in {tried.ms.toFixed(1)} ms</p>
	<pre>{tried.text}</pre>
{/if}

<style>
	.lede {
		color: var(--muted);
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
	}
	.summary {
		margin-left: 0.8rem;
		font-weight: 600;
	}
	.table {
		overflow-x: auto;
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--card);
	}
	table {
		border-collapse: collapse;
		width: 100%;
		font-size: 0.85rem;
	}
	th,
	td {
		padding: 0.5rem 0.65rem;
		border-bottom: 1px solid var(--line);
		text-align: left;
		vertical-align: top;
	}
	th {
		color: var(--muted);
	}
	.pill {
		border-radius: 999px;
		padding: 0.05rem 0.5rem;
		font-weight: 600;
		font-size: 0.8rem;
	}
	.ok {
		color: var(--ok);
		background: var(--ok-bg);
	}
	.bad {
		color: var(--bad);
		background: var(--bad-bg);
	}
	textarea {
		width: 100%;
		box-sizing: border-box;
		font-family: ui-monospace, 'SF Mono', Menlo, monospace;
		font-size: 0.8rem;
		padding: 0.6rem;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--card);
		color: var(--fg);
	}
	pre {
		white-space: pre-wrap;
		word-break: break-all;
		background: var(--card);
		border: 1px solid var(--line);
		border-radius: 8px;
		padding: 0.6rem;
		font-size: 0.75rem;
	}
</style>
