<script lang="ts">
	import PageIntro from '#lib/components/PageIntro.svelte';
	import PerfFigure from '#lib/components/PerfFigure.svelte';
	import Pill from '#lib/components/Pill.svelte';
	import SectionHeading from '#lib/components/SectionHeading.svelte';
	import ApiPanes from '#lib/home/ApiPanes.svelte';
	import FlowDiagram from '#lib/home/FlowDiagram.svelte';
	import RunsChart from '#lib/home/RunsChart.svelte';
	import { reveal } from '#lib/motion.ts';
	import { GITHUB_FILE, LAPTOP_CAVEAT } from '#lib/site.ts';
	import type { PageProps } from './$types';
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

	let { data }: PageProps = $props();
</script>

<Seo
	title="Product · Colophon"
	description="Colophon's features, grouped like the Strapi and Sanity parity backlog, each labelled done, building or planned with its milestone."
/>

<PageIntro badge="Product" title="Everything it does, and everything it will">
	<p>Each feature is done (merged, with its proof), building (a step in progress) or planned (with its milestone).</p>
	{#snippet aside()}
		<dl class="glass grid grid-cols-3 gap-6 rounded-3xl px-8 py-6 text-left">
			{#each KINDS as kind (kind)}
				<div class="space-y-2">
					<dt><StatusBadge {kind} /></dt>
					<dd class="figure text-3xl" title={STATUS_MEANING[kind]}>{tally(kind)}</dd>
				</div>
			{/each}
		</dl>
	{/snippet}
</PageIntro>

<div class="shell mt-16">
	<nav aria-label="Feature groups" class="flex flex-wrap justify-center gap-3">
		{#each FEATURE_GROUPS as group (group.id)}
			<a href="#{group.id}" class="tag outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring">{group.title}</a>
		{/each}
	</nav>

	{#each FEATURE_GROUPS as group, index (group.id)}
		<section aria-labelledby={group.id} class="mt-20 grid gap-8 dt:mt-24 lg:grid-cols-[minmax(0,22rem)_minmax(0,1fr)] lg:gap-12">
			<div class="space-y-4 lg:sticky lg:top-24 lg:self-start">
				<p class="t-eyebrow">0{index + 1}</p>
				<h2 id={group.id} class="t-h2">{group.title}</h2>
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
					<li class="flex flex-col glass rounded-3xl p-5" data-status={feature.status.kind}>
						<StatusBadge kind={feature.status.kind} at={feature.status.at} />
						<h3 class="mt-3 text-[0.9375rem] font-semibold">{feature.name}</h3>
						<p class="mt-1.5 text-sm leading-relaxed text-muted-foreground">{feature.detail}</p>
					</li>
				{/each}
			</ul>
		</section>
	{/each}

	<!-- What a publish does, as a data flow. -->
	<section aria-labelledby="flow" class="section" {@attach reveal}>
		<SectionHeading id="flow" eyebrow="How it works" title="From publish to proof" align="center">
			One request, one transaction, then outside work that survives a crash.
		</SectionHeading>
		<div class="mt-14"><FlowDiagram /></div>
		<p class="sr-only">
			A publish request goes from the Studio, the command line or an agent to colophon-server, which checks permissions and commits release 42 and
			its three delivery jobs in one PostgreSQL transaction. colophon-worker claims each job under a lease and calls out with a stable idempotency
			key: the webhook is delivered, the CDN purge is accepted, and the probe sees release 42 on the live page, so the release is verified at 1
			location.
		</p>
	</section>

	<!-- Real answers from the API. -->
	<section aria-labelledby="api" class="section" {@attach reveal}>
		<SectionHeading id="api" eyebrow="The API" title="Answers that say what they are" align="center">
			Every read names its release; every refusal says why, with a stable code.
		</SectionHeading>
		<div class="mt-14"><ApiPanes /></div>
	</section>

	<!-- Real numbers, each from a committed file. -->
	<section aria-labelledby="proof" class="section" {@attach reveal}>
		<SectionHeading id="proof" eyebrow="Proof, not adjectives" title="Every number comes with its file" align="center">
			All {data.resultCount} load results are committed; {data.failedCount} missed their own thresholds and are kept.
		</SectionHeading>
		<div class="mt-14"><RunsChart runs={data.runs} /></div>
		<div class="mt-6 grid gap-6 tb:grid-cols-2 dt:grid-cols-6">
			<div class="dt:col-span-3"><PerfFigure figure={data.figures.site} emphasis /></div>
			<div class="dt:col-span-3"><PerfFigure figure={data.figures.delivery} emphasis /></div>
			<div class="dt:col-span-2"><PerfFigure figure={data.figures.publishing} /></div>
			<div class="dt:col-span-2"><PerfFigure figure={data.figures.entriesFresh} /></div>
			<div class="dt:col-span-2"><PerfFigure figure={data.figures.entriesLarge} /></div>
		</div>
		<p class="t-small mx-auto mt-6 max-w-3xl text-center">{LAPTOP_CAVEAT}</p>

		<dl class="mt-14 grid gap-6 tb:grid-cols-3">
			<div class="sand rounded-3xl p-8">
				<dt class="t-body">Planted bugs in the publishing simulator, each caught within 200 seeds</dt>
				<dd class="figure t-h3 mt-3">8</dd>
				<dd class="t-small mt-2">D-047, <a class="underline underline-offset-2" href={GITHUB_FILE('docs/modules/10-sim.md')}>module 10</a></dd>
			</div>
			<div class="sand rounded-3xl p-8">
				<dt class="t-body">Seeds of the save simulator on every pull request, 400 steps each</dt>
				<dd class="figure t-h3 mt-3">2,000</dd>
				<dd class="t-small mt-2">D-039, <a class="underline underline-offset-2" href={GITHUB_FILE('.github/workflows/ci.yml')}>CI workflow</a></dd>
			</div>
			<div class="sand rounded-3xl p-8">
				<dt class="t-body">Times the crash loop kills the server and worker in every CI run</dt>
				<dd class="figure t-h3 mt-3">12</dd>
				<dd class="t-small mt-2">D-047, <span class="font-mono">tests/crash_loop.rs</span></dd>
			</div>
		</dl>
	</section>

	<section aria-labelledby="next" class="section glass flex flex-col items-start justify-between gap-6 rounded-3xl p-8 tb:flex-row tb:items-center">
		<div>
			<h2 id="next" class="t-h5">When does the planned part arrive?</h2>
			<p class="t-body mt-2">The roadmap orders the milestones and estimates them, as guesses, at about 9 to 12 months to v0.1.</p>
		</div>
		<Pill label="See the roadmap" href="/roadmap/" />
	</section>
</div>
