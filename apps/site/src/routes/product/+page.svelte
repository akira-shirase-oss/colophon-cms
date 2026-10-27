<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import { Button } from '@colophon/ui/ui/button';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { FEATURE_GROUPS } from '#lib/data/features.ts';
	import { STATUS_MEANING, type StatusKind } from '#lib/status.ts';
	import CrashGraphic from '#lib/bento/CrashGraphic.svelte';
	import DiffGraphic from '#lib/bento/DiffGraphic.svelte';
	import MigrationGraphic from '#lib/bento/MigrationGraphic.svelte';
	import ProbeGraphic from '#lib/bento/ProbeGraphic.svelte';
	import RolesGraphic from '#lib/bento/RolesGraphic.svelte';
	import SliderGraphic from '#lib/bento/SliderGraphic.svelte';
	import Stage from '#lib/bento/Stage.svelte';
	import Terminal from '#lib/components/Terminal.svelte';

	const all = FEATURE_GROUPS.flatMap((group) => group.features);
	const tally = (kind: StatusKind) => all.filter((feature) => feature.status.kind === kind).length;
	const KINDS: StatusKind[] = ['done', 'building', 'planned'];
</script>

<Seo
	title="Product · Colophon"
	description="Colophon's features, grouped like the Strapi and Sanity parity backlog, each labelled done, building or planned with its milestone."
/>

<PageIntro badge="Product" title="Everything it does, and everything it will">
	<p>Each feature is done (merged, with its proof), building (a step in progress) or planned (with its milestone).</p>
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
				<div class="grid gap-3 pt-2">
					{#if group.id === 'writing'}
						<Stage alt="colophon-server is killed with kill -9 while version 7 is saving; after the restart, the retry with the same key is saved once."><CrashGraphic /></Stage>
					{:else if group.id === 'modelling'}
						<div><StatusBadge kind="building" at="3.6" /></div>
						<Stage alt="making a field required is refused with 422 needs_migration, naming the stored entries in the way."><MigrationGraphic /></Stage>
					{:else if group.id === 'publishing'}
						<Stage alt="a probe finds the release marker on the live site: Verified at 1 location." class="h-52"><ProbeGraphic /></Stage>
						<Stage alt="an approval bound to version 7 goes stale when version 8 is saved: 409 stale_changeset." class="h-52"><DiffGraphic /></Stage>
					{:else if group.id === 'time-travel'}
						<div><StatusBadge kind="planned" at="P4" /></div>
						<Stage alt="a release slider" interactive class="h-52"><SliderGraphic /></Stage>
					{:else if group.id === 'access'}
						<div><StatusBadge kind="planned" at="R" /></div>
						<Stage alt="a permission matrix for a custom role, toggling cells."><RolesGraphic /></Stage>
					{:else if group.id === 'developers'}
						<Terminal
							title="Public reads, from any HTTP client"
							lines={[
								{ cmd: 'curl -i "$CMS/content/v1/$PROJECT/production/articles?limit=20"' },
								{ out: 'HTTP/1.1 200 OK' },
								{ out: 'colophon-generation: 42' },
								{ out: 'etag: "8f14e45fceea167a5a36dedd4bea2543"' },
								{ out: 'colophon-cache: hit' },
								{ out: '{"data": [ … ], "next_cursor": "…"}' }
							]}
						/>
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
