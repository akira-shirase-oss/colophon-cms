<!--
  Custom roles (milestone R, D-058; done in R.2 and R.3): a role's permissions as a matrix of
  collections and actions, with "own" for entries the person created (an `own` condition on
  content.update, which R.3 allows). A pointer toggles two cells, slowly, in a nine-second loop.
  The Studio's own grid says See, Create, Edit, Approve and Publish; this one shows the API's
  action names.
-->
<script lang="ts">
	const ACTIONS = ['read', 'create', 'update', 'publish'];
	// 'flip' cells change during the loop; the resting frame shows their first value.
	const ROWS: { name: string; cells: ('yes' | 'no' | 'own' | 'flip-on' | 'flip-own')[] }[] = [
		{ name: 'articles', cells: ['yes', 'yes', 'flip-own', 'no'] },
		{ name: 'pages', cells: ['yes', 'no', 'no', 'no'] },
		{ name: 'authors', cells: ['yes', 'yes', 'yes', 'flip-on'] }
	];
</script>

<div class="flex h-full items-center justify-center p-5">
	<div class="w-full max-w-sm rounded-2xl border border-border bg-card p-4 shadow-soft">
		<p class="font-mono text-[0.6875rem] text-muted-foreground">role: contributor</p>
		<table class="mt-2 w-full text-xs">
			<thead>
				<tr class="text-muted-foreground">
					<th class="pb-1.5 text-left font-medium"></th>
					{#each ACTIONS as action (action)}<th class="pb-1.5 text-center font-mono font-normal">{action}</th>{/each}
				</tr>
			</thead>
			<tbody class="divide-y divide-border border-t border-border">
				{#each ROWS as row (row.name)}
					<tr>
						<th class="py-2 text-left font-mono font-medium">{row.name}</th>
						{#each row.cells as cell, index (index)}
							<td class="py-2 text-center">
								{#if cell === 'flip-own'}
									<span class="cell"><span class="a live text-live-ink">✓</span><span class="b live chip">own</span><span class="ring live"></span></span>
								{:else if cell === 'flip-on'}
									<span class="cell"><span class="a live text-muted-foreground">–</span><span class="b live text-live-ink">✓</span><span class="ring live late"></span></span>
								{:else if cell === 'own'}
									<span class="chip">own</span>
								{:else if cell === 'yes'}
									<span class="text-live-ink">✓</span>
								{:else}
									<span class="text-muted-foreground">–</span>
								{/if}
							</td>
						{/each}
					</tr>
				{/each}
			</tbody>
		</table>
	</div>
</div>

<style>
	.chip {
		border-radius: 999px;
		background: var(--brand-soft);
		color: var(--brand-ink);
		padding: 0 0.375rem;
		font-weight: 500;
	}
	.cell {
		position: relative;
		display: inline-grid;
		place-items: center;
		min-width: 2.25rem;
	}
	.cell > * {
		grid-area: 1 / 1;
	}
	.b,
	.ring {
		opacity: 0;
	}
	.ring {
		width: 2.25rem;
		height: 1.5rem;
		border-radius: 0.5rem;
		border: 1.5px solid var(--brand);
	}
	:global([data-run]) .a {
		animation: a 9s ease infinite both;
	}
	:global([data-run]) .b {
		animation: b 9s ease infinite both;
	}
	:global([data-run]) .ring {
		animation: ring 9s ease infinite both;
	}
	:global([data-run]) .late {
		animation-delay: 1.8s;
	}
	:global([data-run]) td:has(.late) .a,
	:global([data-run]) td:has(.late) .b {
		animation-delay: 1.8s;
	}
	@keyframes a {
		0%, 24% { opacity: 1; transform: scale(1); }
		28%, 70% { opacity: 0; transform: scale(0.7); }
		74%, 100% { opacity: 1; transform: scale(1); }
	}
	@keyframes b {
		0%, 24% { opacity: 0; transform: scale(0.7); }
		28%, 70% { opacity: 1; transform: scale(1); }
		74%, 100% { opacity: 0; transform: scale(0.7); }
	}
	@keyframes ring {
		0%, 14% { opacity: 0; transform: scale(1.2); }
		20%, 32% { opacity: 1; transform: scale(1); }
		38%, 62% { opacity: 0; }
		66%, 76% { opacity: 1; transform: scale(1); }
		82%, 100% { opacity: 0; transform: scale(1.2); }
	}
</style>
