<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Button } from '@colophon/ui/ui/button';
	import Inline from '#lib/components/Inline.svelte';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import ParityGraphic from '#lib/bento/ParityGraphic.svelte';
	import Stage from '#lib/bento/Stage.svelte';
	import { CONVEX, DIFFERENT_ON_PURPOSE, EVIDENCE, PARITY, TABLES, parityCounts } from '#lib/data/compare.ts';
	import { GITHUB_FILE } from '#lib/site.ts';
	import type { StatusKind } from '#lib/status.ts';

	const tally = (kind: StatusKind) => PARITY.filter((row) => row.kind === kind).length;
	const counts = parityCounts();
	// The backlog's filter: every row, or only one status.
	const FILTERS: { value: StatusKind | 'all'; label: string }[] = [
		{ value: 'all', label: 'All' },
		{ value: 'done', label: 'Done' },
		{ value: 'building', label: 'Building' },
		{ value: 'planned', label: 'Planned' }
	];
	let filter = $state<StatusKind | 'all'>('all');
	const shown = $derived(PARITY.filter((row) => filter === 'all' || row.kind === filter));
</script>

<Seo
	title="Colophon vs Strapi vs Sanity"
	description="What Strapi and Sanity offer, how Colophon does it or will, and where its invariants make it work differently. Checked against vendor docs on 2026-10-27."
/>

<PageIntro badge="Compare" title="Colophon, Strapi and Sanity">
	<p>
		The goal since D-057: every feature Strapi and Sanity offer, at Colophon’s bar of proof. {tally('done')} of {PARITY.length} backlog rows are
		done, {tally('building')} building, {tally('planned')} planned.
	</p>
	{#snippet aside()}
		<div class="w-full lg:w-[26rem]">
			<Stage alt="the parity backlog by status, and its planned rows by milestone." class="h-48"><ParityGraphic {...counts} /></Stage>
		</div>
	{/snippet}
</PageIntro>

<div class="shell mt-10">
	<aside aria-labelledby="evidence" class="rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-8">
		<h2 id="evidence" class="title">What this is based on</h2>
		<ul class="mt-4 grid gap-2 text-[0.9375rem] leading-relaxed text-muted-foreground">
			<li><span class="font-medium text-foreground">Checked {EVIDENCE.checked}.</span> Strapi: {EVIDENCE.strapi}. Sanity: {EVIDENCE.sanity}.</li>
			<li>{EVIDENCE.note}</li>
			<li>{EVIDENCE.backlog}</li>
			<li>{EVIDENCE.reconciled}</li>
		</ul>
		<p class="mt-4 text-sm">
			Source: <a class="text-link" href={GITHUB_FILE('docs/COMPARISON.md')}>docs/COMPARISON.md</a>, the list the lettered milestones of the
			<a class="text-link" href="/roadmap/">roadmap</a> work through.
		</p>
	</aside>

	<nav aria-label="On this page" class="mt-10 flex flex-wrap gap-2">
		{#each TABLES as table (table.id)}
			<Button variant="outline" size="sm" href="#{table.id}" class="h-auto min-h-9 max-w-full shrink py-2 text-left whitespace-normal">{table.title}</Button>
		{/each}
		<Button variant="outline" size="sm" href="#different">Different on purpose</Button>
		<Button variant="outline" size="sm" href="#backlog">Parity backlog</Button>
		<Button variant="outline" size="sm" href="#convex">From Convex</Button>
	</nav>

	{#each TABLES as table (table.id)}
		<section aria-labelledby={table.id} class="mt-16">
			<h2 id={table.id} class="display-2">{table.title}</h2>
			{#if table.lead}<p class="mt-3 max-w-3xl text-[0.9375rem] leading-relaxed text-muted-foreground">{table.lead}</p>{/if}
			<!-- A table wider than a phone scrolls sideways; a scrollable region must be reachable by keyboard (axe: scrollable-region-focusable). -->
			<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
			<div class="mt-6 overflow-x-auto rounded-3xl border border-border/70 bg-card shadow-soft" role="region" aria-label="{table.title}, scrollable" tabindex="0">
				<table class="w-full min-w-[46rem] text-left text-sm">
					<thead class="border-b border-border bg-muted/50 text-xs text-muted-foreground">
						<tr>
							{#each table.columns as column, index (index)}
								<th scope="col" class="px-4 py-3 font-medium {column === 'Colophon' ? 'text-foreground' : ''}">{column || 'Feature'}</th>
							{/each}
						</tr>
					</thead>
					<tbody class="divide-y divide-border align-top">
						{#each table.rows as row (row[0])}
							<tr>
								{#each row as cell, index (index)}
									{#if index === 0}
										<th scope="row" class="w-40 px-4 py-3.5 font-medium">{cell}</th>
									{:else}
										<td class="px-4 py-3.5 leading-relaxed {table.columns[index] === 'Colophon' ? 'bg-brand-soft/40' : 'text-muted-foreground'}">
											<Inline text={cell} />
										</td>
									{/if}
								{/each}
							</tr>
						{/each}
					</tbody>
				</table>
			</div>
		</section>
	{/each}

	<section aria-labelledby="different" class="mt-20">
		<h2 id="different" class="display-2">Done differently, on purpose</h2>
		<ul class="mt-6 grid gap-4 md:grid-cols-3">
			{#each DIFFERENT_ON_PURPOSE as item (item.title)}
				<li class="rounded-3xl border border-border/70 bg-card p-6 shadow-hairline">
					<h3 class="text-[0.9375rem] font-semibold">{item.title}</h3>
					<p class="mt-2 text-sm leading-relaxed text-muted-foreground">{item.text}</p>
				</li>
			{/each}
		</ul>
	</section>

	<section aria-labelledby="backlog" class="mt-20">
		<h2 id="backlog" class="display-2">The parity backlog</h2>
		<p class="mt-3 max-w-3xl text-[0.9375rem] leading-relaxed text-muted-foreground">Who offers what today, and when Colophon builds it. {EVIDENCE.backlog}</p>
		<div role="group" aria-label="Show rows" class="mt-5 inline-flex flex-wrap gap-1 rounded-full border border-border/70 bg-card p-1 shadow-hairline">
			{#each FILTERS as option (option.value)}
				<button
					type="button"
					aria-pressed={filter === option.value}
					onclick={() => (filter = option.value)}
					class="rounded-full px-3.5 py-1.5 text-sm font-medium text-muted-foreground outline-none hover:text-foreground focus-visible:ring-4 focus-visible:ring-ring/25 aria-pressed:bg-primary aria-pressed:text-primary-foreground"
				>
					{option.label}<span class="figure ml-1.5 text-xs opacity-100">{option.value === 'all' ? PARITY.length : tally(option.value)}</span>
				</button>
			{/each}
		</div>
		<!-- A table wider than a phone scrolls sideways; a scrollable region must be reachable by keyboard (axe: scrollable-region-focusable). -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<div class="mt-6 overflow-x-auto rounded-3xl border border-border/70 bg-card shadow-soft" role="region" aria-label="The parity backlog, scrollable" tabindex="0">
			<table class="w-full min-w-[44rem] text-left text-sm" data-testid="parity">
				<thead class="border-b border-border bg-muted/50 text-xs text-muted-foreground">
					<tr>
						<th scope="col" class="px-4 py-3 font-medium">Feature</th>
						<th scope="col" class="px-4 py-3 font-medium">Strapi</th>
						<th scope="col" class="px-4 py-3 font-medium">Sanity</th>
						<th scope="col" class="px-4 py-3 font-medium text-foreground">Colophon</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-border align-top">
					{#each shown as row (row.feature)}
						<tr data-kind={row.kind}>
							<th scope="row" class="px-4 py-3 font-medium">{row.feature}</th>
							<td class="px-4 py-3 text-muted-foreground">{#if row.strapi === '–'}<span aria-label="Not offered">–</span>{:else}{row.strapi}{/if}</td>
							<td class="px-4 py-3 text-muted-foreground">{#if row.sanity === '–'}<span aria-label="Not offered">–</span>{:else}{row.sanity}{/if}</td>
							<td class="px-4 py-3">
								<StatusBadge kind={row.kind} />
								<span class="mt-1 block text-xs text-muted-foreground">{row.colophon}</span>
							</td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
		<p class="mt-3 text-xs text-muted-foreground">✓ offered; – not offered; “by plan” and “paid” are plan tiers, from general knowledge.</p>
	</section>

	<section aria-labelledby="convex" class="mt-20">
		<h2 id="convex" class="display-2">What Colophon borrows from Convex</h2>
		<p class="mt-3 max-w-3xl text-[0.9375rem] leading-relaxed text-muted-foreground">
			Convex is not a CMS, but its engine is built for fast, always-consistent reads, and several of its ideas fit Colophon’s invariants. Read on
			2026-10-27 from Convex’s own write-up and docs and a self-hosted demo. Not taken: running user code inside the database, and a custom
			transaction log; PostgreSQL stays the authority.
		</p>
		<ul class="mt-6 grid gap-3 md:grid-cols-2">
			{#each CONVEX as item (item.idea)}
				<li class="rounded-3xl border border-border/70 bg-card p-5 shadow-hairline">
					<p class="text-xs text-muted-foreground">{item.when.startsWith('done') ? 'Already: ' + item.when : 'Milestone ' + item.when}</p>
					<h3 class="mt-1 text-[0.9375rem] font-semibold">{item.idea}</h3>
					<p class="mt-1.5 text-sm leading-relaxed text-muted-foreground">{item.colophon}</p>
				</li>
			{/each}
		</ul>
		<p class="mt-8 text-sm text-muted-foreground">
			Something wrong or out of date here? The comparison is a file in the repository:
			<a class="inline-flex items-center gap-0.5 font-medium text-brand-ink underline underline-offset-4" href={GITHUB_FILE('docs/COMPARISON.md')}
				>docs/COMPARISON.md<ArrowUpRightIcon class="size-3.5" aria-hidden="true" /></a
			>.
		</p>
	</section>
</div>
