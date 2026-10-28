<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleIcon from '@lucide/svelte/icons/circle';
	import LoaderIcon from '@lucide/svelte/icons/loader';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { MILESTONES, TOTAL_ESTIMATE } from '#lib/data/roadmap.ts';
	import { GITHUB_FILE } from '#lib/site.ts';

	const done = MILESTONES.filter((milestone) => milestone.kind === 'done').length;
	const underWay = MILESTONES.filter((milestone) => milestone.kind === 'building').map((milestone) => milestone.id);
	const NUMBERS = ['no', 'one', 'two', 'three', 'four', 'five', 'six', 'seven', 'eight', 'nine', 'ten', 'eleven', 'twelve', 'thirteen', 'fourteen', 'fifteen'];
	const count = NUMBERS[MILESTONES.length] ?? String(MILESTONES.length);
	const Count = count[0]!.toUpperCase() + count.slice(1);
	const list = (ids: string[]) => (ids.length < 2 ? (ids[0] ?? '') : `${ids.slice(0, -1).join(', ')} and ${ids.at(-1)}`);
	const stepWord = (step: { kind: string; note?: string }) =>
		(step.kind === 'done' ? 'done' : step.kind === 'building' ? 'building' : 'planned') + (step.note ? `, ${step.note}` : '');
</script>

<Seo
	title="Roadmap · Colophon"
	description="Colophon's {count} milestones, each named after what it proves and done only when its proof gate passes with stored evidence."
/>

<PageIntro badge="Roadmap" title="{Count} milestones, each named after what it proves">
	<p>
		A milestone is done when its proof gate passes with stored evidence, not when its screens exist. {done} of {MILESTONES.length} are done;
		{list(underWay)} {underWay.length === 1 ? 'is' : 'are'} under way. In all, {TOTAL_ESTIMATE}. Estimates assume about 45 hours a week and are
		guesses, not measurements.
	</p>
</PageIntro>

<div class="shell mt-12">
	<ol class="relative grid gap-5">
		{#each MILESTONES as milestone (milestone.id)}
			<li id={milestone.id.toLowerCase()} data-testid="milestone" data-status={milestone.kind}>
				<article
					class="grid gap-6 glass rounded-3xl border p-6 sm:p-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)] {milestone.kind === 'building'
						? 'border-pending ring-4 ring-pending-soft'
						: 'border-transparent'}"
				>
					<div>
						<div class="flex flex-wrap items-center gap-3">
							<span class="figure grid size-11 place-items-center rounded-2xl text-[0.9375rem] font-semibold {milestone.kind === 'done'
								? 'bg-primary text-primary-foreground'
								: milestone.kind === 'building'
									? 'bg-pending-soft text-pending-ink'
									: 'bg-muted text-muted-foreground'}"
							>
								{milestone.id}
							</span>
							<StatusBadge kind={milestone.kind} at={milestone.label} of="milestone" />
							<span class="text-sm text-muted-foreground">Estimate: {milestone.estimate}</span>
						</div>
						<h2 class="t-h2 mt-4">{milestone.name}</h2>
						<p class="mt-3 text-[0.9375rem] leading-relaxed text-muted-foreground">{milestone.goal}</p>
						<div class="mt-5 rounded-2xl border border-border bg-background p-4">
							<h3 class="eyebrow">Proof gate</h3>
							<p class="mt-1.5 text-sm leading-relaxed">{milestone.gate}</p>
						</div>
					</div>
					<div>
						{#if milestone.steps}
							<h3 class="text-sm font-semibold">Steps</h3>
							<ol class="mt-3 grid gap-1.5">
								{#each milestone.steps as step (step.id)}
									<li class="flex items-start gap-3 rounded-xl px-3 py-2 text-sm {step.kind === 'building' ? 'bg-pending-soft text-pending-ink' : ''}">
										{#if step.kind === 'done'}
											<CheckIcon class="mt-0.5 size-4 shrink-0 text-live-ink" aria-hidden="true" />
										{:else if step.kind === 'building'}
											<LoaderIcon class="mt-0.5 size-4 shrink-0" aria-hidden="true" />
										{:else}
											<CircleIcon class="mt-0.5 size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
										{/if}
										<span class="figure w-8 shrink-0 font-medium">{step.id}</span>
										<span class="flex-1">
											{step.title}
											<span class="sr-only">({stepWord(step)})</span>
											{#if step.note}<span aria-hidden="true" class="ml-1 text-xs font-medium">· {step.note}</span>{/if}
										</span>
									</li>
								{/each}
							</ol>
						{:else if milestone.bullets}
							{#if milestone.built}
								<h3 class="text-sm font-semibold">Done so far</h3>
								<ul class="mt-3 mb-6 grid gap-2">
									{#each milestone.built as item (item)}
										<li class="flex items-start gap-3 text-sm leading-relaxed">
											<CheckIcon class="mt-0.5 size-4 shrink-0 text-live-ink" aria-hidden="true" />{item}<span class="sr-only"> (done)</span>
										</li>
									{/each}
								</ul>
							{/if}
							<h3 class="text-sm font-semibold">{milestone.built ? 'Still to build' : 'What it builds'}</h3>
							<ul class="mt-3 grid gap-2">
								{#each milestone.bullets as bullet (bullet)}
									<li class="flex items-start gap-3 text-sm leading-relaxed text-muted-foreground">
										<span class="mt-2 size-1.5 shrink-0 rounded-full bg-draft" aria-hidden="true"></span>{bullet}
									</li>
								{/each}
							</ul>
						{/if}
					</div>
				</article>
			</li>
		{/each}
	</ol>

	<p class="mt-10 max-w-3xl text-sm leading-relaxed text-muted-foreground">
		After v0.1: a business-site template with forms, and possibly a commerce showcase. Never: untrusted native plugins, active-active
		multi-region writes. The full plan, with each milestone’s module list, is
		<a class="text-link" href={GITHUB_FILE('docs/ROADMAP.md')}>docs/ROADMAP.md</a>; the order of R, M, E and D comes from D-057, and S and V from
		D-062.
	</p>
</div>
