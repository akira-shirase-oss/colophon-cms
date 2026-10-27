<!--
  The reference's three pricing cards, used for something true: Colophon has no prices (it is
  open source and pre-release, with no hosted version), so the cards say where it stands. Done on
  the left, the milestone under way in the raised middle card, what remains on the right. The
  middle card's switch (the reference's Annually/Monthly) shows now or next. From
  src/lib/data/roadmap.ts.
-->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleIcon from '@lucide/svelte/icons/circle';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import Pill from '#lib/components/Pill.svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { MILESTONES } from '#lib/data/roadmap.ts';
	import { reveal } from '#lib/motion.ts';
	import { STATUS_WORD, type StatusKind } from '#lib/status.ts';

	const byId = (id: string) => MILESTONES.find((milestone) => milestone.id === id)!;
	const done = MILESTONES.filter((milestone) => milestone.kind === 'done');
	const current = MILESTONES.find((milestone) => milestone.kind === 'building') ?? byId('P3');
	const next = MILESTONES[MILESTONES.indexOf(current) + 1] ?? byId('R');
	const later = MILESTONES.slice(MILESTONES.indexOf(next) + 1);

	type Item = { text: string; kind: StatusKind };
	// The roadmap's step titles, shortened to fit a line; anything not listed keeps its title.
	const SHORT: Record<string, string> = {
		'3.1': 'A cache that can’t serve the wrong thing',
		'3.4': 'A site built from one release',
		'3.5': 'The Studio, designed',
		'3.6': 'Slugs and the type builder',
		'3.7': 'The non-technical path',
		'3.8': 'Live site and a public demo',
		'R.1': 'Roles as data',
		'R.2': 'Custom roles and their editor',
		'R.3': 'Field-level permissions',
		'R.4': 'API tokens and an audit log',
		M: 'Content modelling',
		E: 'Editorial work',
		D: 'Developer APIs'
	};
	const steps = (id: string): Item[] => (byId(id).steps ?? []).map((step) => ({ text: SHORT[step.id] ?? step.title, kind: step.kind }));

	const DONE_ITEMS: Item[] = [
		{ text: 'Drafts that can’t be lost', kind: 'done' },
		{ text: 'Approving the exact change', kind: 'done' },
		{ text: 'One release, one number', kind: 'done' },
		{ text: 'Work that survives a crash', kind: 'done' },
		{ text: 'Seeded simulation in CI', kind: 'done' }
	];
	// Five lines, like its neighbours: the last two milestones share one.
	const LATER_ITEMS: Item[] = [
		...later.slice(0, -2).map((milestone) => ({ text: `${SHORT[milestone.id] ?? milestone.name} · ${milestone.id}`, kind: milestone.kind })),
		...(later.length >= 2 ? [{ text: `Automation, ops · ${later.at(-2)!.id}, ${later.at(-1)!.id}`, kind: later.at(-1)!.kind }] : [])
	];

	let showing = $state<'now' | 'next'>('now');
	const middle = $derived(showing === 'now' ? current : next);
	const middleItems = $derived(showing === 'now' ? steps(current.id).slice(-5) : steps(next.id).slice(0, 5));
</script>

{#snippet checklist(items: Item[])}
	<ul class="mt-5 grid gap-4">
		{#each items as item (item.text)}
			<li class="t-body-lg flex items-start gap-3">
				{#if item.kind === 'done'}
					<CheckIcon class="mt-1 size-5 shrink-0 text-foreground" aria-hidden="true" />
				{:else if item.kind === 'building'}
					<LoaderIcon class="mt-1 size-5 shrink-0 text-pending-ink" aria-hidden="true" />
				{:else}
					<CircleIcon class="mt-1 size-5 shrink-0 text-muted-foreground" aria-hidden="true" />
				{/if}
				<span>{item.text}<span class="sr-only"> ({STATUS_WORD[item.kind].toLowerCase()})</span></span>
			</li>
		{/each}
	</ul>
{/snippet}

<section aria-labelledby="where" class="section shell" {@attach reveal}>
	<div class="mx-auto max-w-[50rem] text-center">
		<p class="t-eyebrow mb-5">Roadmap</p>
		<h2 id="where" class="t-h2">Where it stands,<br class="max-tb:hidden" /> and what comes next</h2>
	</div>

	<div class="mt-10 grid items-end gap-6 dt:mt-14 dt:grid-cols-3">
		<article class="glass flex flex-col rounded-3xl p-8" data-status="done">
			<div>
				<p class="text-lg leading-[1.4] font-medium text-ink-2">Milestones P0 to P2</p>
				<p class="t-h3 mt-1">{STATUS_WORD.done}</p>
				<p class="t-body-lg mt-6">Merged, with their proof.</p>
			</div>
			{@render checklist(DONE_ITEMS)}
			<Pill label="Read the changelog" href="/changelog/" variant="secondary" class="mt-8 w-full" />
		</article>

		<article class="highlight flex flex-col rounded-3xl p-8" data-status={middle.kind}>
			<div role="group" aria-label="Show the milestone" class="switch mb-8 grid grid-cols-2 rounded-full p-1">
				<button type="button" aria-pressed={showing === 'now'} onclick={() => (showing = 'now')} class="switch-option">Now</button>
				<button type="button" aria-pressed={showing === 'next'} onclick={() => (showing = 'next')} class="switch-option">Next</button>
			</div>
			<div>
				<p class="flex flex-wrap items-center gap-2 text-lg leading-[1.4] font-medium text-ink-2">
					Milestone {middle.id}
					<StatusBadge kind={middle.kind} />
				</p>
				<p class="t-h3 mt-1">{showing === 'now' ? 'Under way' : 'Up next'}</p>
				<p class="t-body-lg mt-6">{middle.name}.</p>
			</div>
			{@render checklist(middleItems)}
			<Pill label="See the roadmap" href="/roadmap/#{middle.id.toLowerCase()}" class="mt-8 w-full" />
		</article>

		<article class="glass flex flex-col rounded-3xl p-8" data-status="planned">
			<div>
				<p class="text-lg leading-[1.4] font-medium text-ink-2">Milestones {later[0]?.id} to {later.at(-1)?.id}</p>
				<p class="t-h3 mt-1">{STATUS_WORD.planned}</p>
				<p class="t-body-lg mt-6">Parity first, then v0.1.</p>
			</div>
			{@render checklist(LATER_ITEMS)}
			<Pill label="See the comparison" href="/compare/" variant="secondary" class="mt-8 w-full" />
		</article>
	</div>

	<p class="t-body mt-14 text-center text-muted-foreground">
		There are no prices: Colophon is open source and pre-release, with no hosted version and nothing to buy.
	</p>
</section>

<style>
	/* The milestone under way: a flat card like its neighbours, marked by its outline (blue in
	   light, the orange accent in dark; app.css). */
	.highlight {
		background: var(--highlight-fill);
		box-shadow: inset 0 0 0 var(--highlight-width) var(--highlight-line);
		-webkit-backdrop-filter: blur(10px);
		backdrop-filter: blur(10px);
	}
	.switch {
		background: var(--chip);
	}
	.switch-option {
		min-height: 2.5rem;
		border-radius: 100px;
		font-size: 1rem;
		line-height: 1;
		font-weight: 600;
		color: var(--foreground);
		outline: none;
		transition:
			background-color 300ms ease,
			box-shadow 300ms ease;
	}
	.switch-option[aria-pressed='true'] {
		background: var(--card);
		box-shadow: var(--shadow-frame);
	}
	.switch-option:focus-visible {
		box-shadow:
			0 0 0 2px var(--background),
			0 0 0 4px var(--ring);
	}
</style>
