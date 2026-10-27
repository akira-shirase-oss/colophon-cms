<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import { Button } from '@colophon/ui/ui/button';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { FEATURE_GROUPS } from '#lib/data/features.ts';
	import { STATUS_MEANING, type StatusKind } from '#lib/status.ts';
	import EditorCard from '#lib/visuals/EditorCard.svelte';
	import LabelStates from '#lib/visuals/LabelStates.svelte';
	import PublishRequestCard from '#lib/visuals/PublishRequestCard.svelte';
	import ReleaseSlider from '#lib/visuals/ReleaseSlider.svelte';
	import RolesMatrix from '#lib/visuals/RolesMatrix.svelte';

	const all = FEATURE_GROUPS.flatMap((group) => group.features);
	const tally = (kind: StatusKind) => all.filter((feature) => feature.status.kind === kind).length;
	const KINDS: StatusKind[] = ['done', 'building', 'planned'];
</script>

<Seo
	title="Product · Colophon"
	description="Colophon's features, grouped like the Strapi and Sanity parity backlog, each labelled done, building or planned with its milestone."
/>

<PageIntro badge="Product" title="Everything it does, and everything it will">
	<p>
		Grouped like the parity backlog in the comparison with Strapi and Sanity. Each feature carries one label: done means merged with its tests, a
		page in the test app and a load scenario; building means a step in progress; planned names the milestone that will build it.
	</p>
	{#snippet aside()}
		<dl class="grid grid-cols-3 gap-3 rounded-3xl border border-border/70 bg-card p-5 shadow-soft">
			{#each KINDS as kind (kind)}
				<div class="space-y-2">
					<dt><StatusBadge {kind} /></dt>
					<dd class="figure text-3xl" title={STATUS_MEANING[kind]}>{tally(kind)}</dd>
				</div>
			{/each}
		</dl>
	{/snippet}
</PageIntro>

<div class="shell mt-10">
	<nav aria-label="Feature groups" class="flex flex-wrap gap-2">
		{#each FEATURE_GROUPS as group (group.id)}
			<Button variant="outline" size="sm" href="#{group.id}">{group.title}</Button>
		{/each}
	</nav>

	{#each FEATURE_GROUPS as group, index (group.id)}
		<section aria-labelledby={group.id} class="mt-20 grid gap-8 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)] lg:gap-12">
			<div class="space-y-4 lg:sticky lg:top-24 lg:self-start">
				<p class="eyebrow text-brand-ink">0{index + 1}</p>
				<h2 id={group.id} class="display-2">{group.title}</h2>
				<p class="text-[0.9375rem] leading-relaxed text-pretty text-muted-foreground">{group.lead}</p>
				<div class="hidden pt-4 lg:block">
					{#if group.id === 'writing'}
						<EditorCard />
					{:else if group.id === 'publishing'}
						<PublishRequestCard />
						<LabelStates class="mt-4" />
					{:else if group.id === 'time-travel'}
						<ReleaseSlider />
						<p class="mt-2 text-xs text-muted-foreground">Illustration of a planned screen.</p>
					{:else if group.id === 'access'}
						<RolesMatrix />
					{/if}
				</div>
			</div>
			<ul class="grid gap-3 sm:grid-cols-2">
				{#each group.features as feature (feature.name)}
					<li class="flex flex-col rounded-3xl border border-border/70 bg-card p-5 shadow-hairline" data-status={feature.status.kind}>
						<StatusBadge kind={feature.status.kind} at={feature.status.at} />
						<h3 class="mt-3 text-[0.9375rem] font-semibold">{feature.name}</h3>
						<p class="mt-1.5 text-sm leading-relaxed text-muted-foreground">{feature.detail}</p>
					</li>
				{/each}
			</ul>
		</section>
	{/each}

	<section aria-labelledby="next" class="mt-24 flex flex-col items-start justify-between gap-6 rounded-[2rem] border border-border/70 bg-card p-8 shadow-soft sm:flex-row sm:items-center">
		<div>
			<h2 id="next" class="title">When does the planned part arrive?</h2>
			<p class="mt-1 text-[0.9375rem] text-muted-foreground">The roadmap orders the milestones and estimates them, as guesses, at about 9 to 12 months to v0.1.</p>
		</div>
		<Button href="/roadmap/">See the roadmap<ArrowRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
	</section>
</div>
