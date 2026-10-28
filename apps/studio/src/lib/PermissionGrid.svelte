<!--
  What a role or a custom API token may do, as a grid in plain words (D-058): a row for everything
  and one per content type, a column per kind of work, and below it the powers over the whole
  project that the caller offers. A tick in "Everything" covers every content type, including ones
  added later. The role editor puts its limited permissions between the two (`between`).
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { Action, ContentType } from './api.ts';
	import { cell, POWERS, WORK } from './permissions.ts';

	let {
		types,
		grants = $bindable(),
		editable,
		powers = POWERS.map((power) => power.action),
		between,
		label = 'Content'
	}: {
		types: ContentType[];
		/** Ticked cells, as `action|subject`. */
		grants: Set<string>;
		editable: boolean;
		/** Which whole-project powers to offer. */
		powers?: Action[];
		between?: Snippet;
		/** The grid's heading. */
		label?: string;
	} = $props();

	const rows = $derived([
		{ subject: '*', name: 'Everything', hint: 'Every content type, including ones added later.' },
		...types.map((type) => ({ subject: type.slug, name: type.definition.name, hint: '' }))
	]);
	const offered = $derived(POWERS.filter((power) => powers.includes(power.action)));
	const id = $props.id();

	function toggle(action: Action, subject: string, on: boolean) {
		const next = new Set(grants);
		if (on) next.add(cell(action, subject));
		else next.delete(cell(action, subject));
		grants = next;
	}
</script>

<h2 class="mt-8 mb-1 text-base font-semibold" id="{id}-work">{label}</h2>
<p class="mb-4 text-sm text-muted-foreground">What it may do with entries, and with each content type's fields.</p>
<div class="overflow-x-auto rounded-xl border border-border bg-card">
	<table class="w-full text-sm" aria-labelledby="{id}-work" data-testid="role-grid">
		<thead>
			<tr class="border-b border-border">
				<th scope="col" class="px-4 py-2.5 text-left font-medium text-muted-foreground"><span class="sr-only">Content type</span></th>
				{#each WORK as column (column.action)}
					<th scope="col" class="px-2 py-2.5 text-center font-medium whitespace-nowrap">{column.label}</th>
				{/each}
			</tr>
		</thead>
		<tbody>
			{#each rows as row (row.subject)}
				<tr class="rule-dotted last:border-b-0">
					<th scope="row" class="px-4 py-2.5 text-left font-normal">
						<span class="font-medium">{row.name}</span>
						{#if row.hint}<span class="block text-xs text-muted-foreground">{row.hint}</span>{/if}
					</th>
					{#each WORK as column (column.action)}
						{@const covered = row.subject !== '*' && grants.has(cell(column.action, '*'))}
						<td class="px-2 py-2.5 text-center">
							<input
								type="checkbox"
								class="size-4 accent-primary disabled:opacity-60"
								aria-label="{column.label}: {row.name}"
								title={covered ? 'Included in Everything' : undefined}
								checked={covered || grants.has(cell(column.action, row.subject))}
								disabled={!editable || covered}
								onchange={(event) => toggle(column.action, row.subject, event.currentTarget.checked)}
							/>
						</td>
					{/each}
				</tr>
			{/each}
		</tbody>
	</table>
</div>

{@render between?.()}

{#if offered.length > 0}
	<h2 class="mt-8 mb-1 text-base font-semibold">The whole project</h2>
	<p class="mb-4 text-sm text-muted-foreground">Powers that aren't about one content type.</p>
	<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="role-project">
		{#each offered as power (power.action)}
			<li class="rule-dotted last:border-b-0">
				<label class="flex items-start gap-3 px-4 py-3">
					<input
						type="checkbox"
						class="mt-0.5 size-4 accent-primary disabled:opacity-60"
						checked={grants.has(cell(power.action, '*'))}
						disabled={!editable}
						onchange={(event) => toggle(power.action, '*', event.currentTarget.checked)}
					/>
					<span>
						<span class="text-sm font-medium">{power.label}</span>
						<span class="block text-xs text-muted-foreground">{power.hint}</span>
					</span>
				</label>
			</li>
		{/each}
	</ul>
{/if}
