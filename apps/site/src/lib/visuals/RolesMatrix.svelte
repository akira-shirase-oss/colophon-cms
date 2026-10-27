<!-- What custom roles will look like (milestone R, D-058): collections × actions, fields, "only their
     own". Planned, so it says so on the picture itself. -->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import MinusIcon from '@lucide/svelte/icons/minus';
	import StatusBadge from '#lib/components/StatusBadge.svelte';

	let { class: className = '' }: { class?: string } = $props();

	const ACTIONS = ['Read', 'Create', 'Update', 'Publish'];
	const ROWS: { collection: string; allowed: (boolean | 'own')[] }[] = [
		{ collection: 'Articles', allowed: [true, true, 'own', false] },
		{ collection: 'Pages', allowed: [true, false, false, false] },
		{ collection: 'Authors', allowed: [true, true, true, false] }
	];
</script>

<figure class="rounded-3xl border border-border/60 bg-card p-5 shadow-float {className}">
	<div class="flex flex-wrap items-center justify-between gap-2">
		<div>
			<p class="text-sm font-semibold">Role: Contributor</p>
			<p class="text-xs text-muted-foreground">A custom role, edited by an owner</p>
		</div>
		<StatusBadge kind="planned" at="R" />
	</div>
	<table class="mt-4 w-full text-sm">
		<caption class="sr-only">An illustration of a planned custom role: what it may do in each collection</caption>
		<thead>
			<tr class="text-xs text-muted-foreground">
				<th scope="col" class="pb-2 text-left font-medium">Collection</th>
				{#each ACTIONS as action (action)}<th scope="col" class="pb-2 text-center font-medium">{action}</th>{/each}
			</tr>
		</thead>
		<tbody class="divide-y divide-border border-t border-border">
			{#each ROWS as row (row.collection)}
				<tr>
					<th scope="row" class="py-2.5 text-left font-medium">{row.collection}</th>
					{#each row.allowed as allowed, index (index)}
						<td class="py-2.5 text-center">
							{#if allowed === 'own'}
								<span class="rounded-full bg-brand-soft px-2 py-0.5 text-[0.6875rem] font-medium text-brand-ink">Own<span class="sr-only">: only their own entries</span></span>
							{:else if allowed}
								<CheckIcon class="mx-auto size-4 text-live-ink" aria-hidden="true" /><span class="sr-only">Allowed</span>
							{:else}
								<MinusIcon class="mx-auto size-4 text-muted-foreground" aria-hidden="true" /><span class="sr-only">Not allowed</span>
							{/if}
						</td>
					{/each}
				</tr>
			{/each}
		</tbody>
	</table>
	<figcaption class="mt-3 text-xs text-muted-foreground">“Own”: only entries the person created. Fields can be limited too.</figcaption>
</figure>
