<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import BentoCard from '#lib/bento/BentoCard.svelte';
	import CrashGraphic from '#lib/bento/CrashGraphic.svelte';
	import DiffGraphic from '#lib/bento/DiffGraphic.svelte';
	import LoadGraphic from '#lib/bento/LoadGraphic.svelte';
	import ParityGraphic from '#lib/bento/ParityGraphic.svelte';
	import ProbeGraphic from '#lib/bento/ProbeGraphic.svelte';
	import RolesGraphic from '#lib/bento/RolesGraphic.svelte';
	import SeedGraphic from '#lib/bento/SeedGraphic.svelte';
	import SliderGraphic from '#lib/bento/SliderGraphic.svelte';
	import PerfFigure from '#lib/components/PerfFigure.svelte';
	import SectionHeading from '#lib/components/SectionHeading.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import Terminal from '#lib/components/Terminal.svelte';
	import ApiPanes from '#lib/home/ApiPanes.svelte';
	import FlowDiagram from '#lib/home/FlowDiagram.svelte';
	import HeroStory from '#lib/home/HeroStory.svelte';
	import RunsChart from '#lib/home/RunsChart.svelte';
	import { DESCRIPTION, GITHUB, GITHUB_FILE, LAPTOP_CAVEAT, QUICKSTART } from '#lib/site.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const load = $derived([
		{ label: 'site reads', rps: data.figures.site.result.rps, p99: data.figures.site.result.latency_ms.p99, ok: true },
		{ label: 'public reads, cached', rps: data.figures.delivery.result.rps, p99: data.figures.delivery.result.latency_ms.p99, ok: true },
		{ label: 'drafts, fresh database', rps: data.figures.entriesFresh.result.rps, p99: data.figures.entriesFresh.result.latency_ms.p99, ok: true },
		{ label: 'publishing, with probes', rps: data.figures.publishing.result.rps, p99: data.figures.publishing.result.latency_ms.p99, ok: true }
	]);
</script>

<Seo title="Colophon: the open-source CMS that can prove what it published" description={DESCRIPTION} />

<!-- The promise, in one sentence, over the product playing it out. -->
<section aria-labelledby="hero-title" class="overflow-hidden">
	<div class="shell pt-14 pb-12 text-center sm:pt-20">
		<a
			href="/roadmap/"
			class="rise inline-flex max-w-full items-center gap-2 rounded-full border border-border bg-card py-1 pr-3 pl-1 text-xs shadow-hairline outline-none hover:bg-accent focus-visible:ring-4 focus-visible:ring-ring/25"
		>
			<Badge variant="pending">Pre-release</Badge>
			<span class="truncate text-muted-foreground">v0.1 is not out yet. See what’s done</span>
			<ArrowRightIcon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
		</a>
		<h1 id="hero-title" class="display-hero rise mx-auto mt-6 max-w-4xl text-balance" style="--i: 1">
			The CMS that can prove what it published.
		</h1>
		<p class="rise mx-auto mt-6 max-w-2xl text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground sm:text-lg" style="--i: 2">
			Open source, in Rust and Svelte. Every publish is one numbered release with a receipt that says where it was seen, and nothing you saved is
			lost when a process dies.
		</p>
		<div class="rise mt-8 flex flex-col items-center justify-center gap-3 sm:flex-row" style="--i: 3">
			<Button size="lg" href={QUICKSTART} rel="external">Read the quickstart<ArrowRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
			<Button size="lg" variant="outline" href={GITHUB}>View on GitHub<ArrowUpRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
		</div>
	</div>

	<div class="shell">
		<HeroStory />
		<p class="mt-3 text-center text-xs text-muted-foreground">An illustration built from the Studio’s real states and words, not data from a real site.</p>
	</div>
</section>

<!-- What a publish does, as a data flow. -->
<section aria-labelledby="flow" class="shell mt-28">
	<SectionHeading id="flow" eyebrow="How it works" title="From publish to proof">
		One request, one transaction, then outside work that survives a crash.
	</SectionHeading>
	<div class="mt-10"><FlowDiagram /></div>
	<p class="sr-only">
		A publish request goes from the Studio, the command line or an agent to colophon-server, which checks permissions and commits release 42 and its
		three delivery jobs in one PostgreSQL transaction. colophon-worker claims each job under a lease and calls out with a stable idempotency key: the
		webhook is delivered, the CDN purge is accepted, and the probe sees release 42 on the live page, so the release is verified at 1 location.
	</p>
</section>

<!-- The guarantees, each as a small live picture of real behaviour. -->
<section aria-labelledby="promises" class="shell mt-28">
	<SectionHeading id="promises" eyebrow="What it guarantees" title="Built around what is hard to fake" />
	<div class="mt-10 grid gap-4 md:grid-cols-2 lg:grid-cols-6">
		<BentoCard
			kind="done"
			at="P1, P2"
			title="Kill it at any moment, lose nothing"
			alt="colophon-server is killed with kill -9 while version 7 is saving; it restarts, and the retry with the same key is saved once, with versions 5 and 6 untouched."
			class="md:col-span-2 lg:col-span-4"
			stage="h-60"
		>
			{#snippet graphic()}<CrashGraphic />{/snippet}
			Fault tests F01 to F05 on the real binary, and a crash loop with twelve SIGKILLs per CI run.
		</BentoCard>
		<BentoCard
			kind="done"
			at="P3"
			title="Receipts, not spinners"
			alt="a probe travels to the live site, finds the marker meta colophon-generation 42 and comes back: Verified at 1 location."
			class="lg:col-span-2"
		>
			{#snippet graphic()}<ProbeGraphic />{/snippet}
			“Verified” only once a probe sees the release on the live page, and it says where.
		</BentoCard>
		<BentoCard
			kind="done"
			at="P1, P2"
			title="Every bug is a seed"
			alt="a terminal runs the simulator twice with the same seed; both runs end on the same trace hash."
			class="lg:col-span-2"
		>
			{#snippet graphic()}<SeedGraphic />{/snippet}
			2,000 seeds on every pull request; a failure replays exactly.
		</BentoCard>
		<BentoCard
			kind="done"
			at="every step"
			title="A million requests per feature"
			alt="bars of requests a second from the committed load results: {load.map((row) => `${row.label} ${row.rps} per second`).join(', ')}."
			class="md:col-span-2 lg:col-span-4"
			href="#proof"
		>
			{#snippet graphic()}<LoadGraphic rows={load} />{/snippet}
			Read from <span class="font-mono text-[0.85em]">perf/results</span>, failures kept. Laptop numbers, not a capacity claim.
		</BentoCard>
		<BentoCard
			kind="done"
			at="P2"
			title="Approve the exact change"
			alt="a publish request approved for version 7 by its hash; when version 8 is saved afterwards, acting on the approval returns 409 stale_changeset."
			class="lg:col-span-3"
		>
			{#snippet graphic()}<DiffGraphic />{/snippet}
			An approval is bound to a hash of the versions; edit one and it is stale.
		</BentoCard>
		<BentoCard
			kind="planned"
			at="P4"
			title="Time travel"
			alt="a release slider"
			class="lg:col-span-3"
			interactive
		>
			{#snippet graphic()}<SliderGraphic />{/snippet}
			Reads pinned to any release work today; the slider and rollback come in P4.
		</BentoCard>
		<BentoCard
			kind="building"
			at="R"
			title="Roles you define"
			alt="a permission matrix for a custom role; cells toggle between allowed, not allowed and only their own."
			class="lg:col-span-2"
		>
			{#snippet graphic()}<RolesGraphic />{/snippet}
			Roles are data now; custom roles, field-level and “own entries” rules, API tokens and an audit log come next.
		</BentoCard>
		<BentoCard
			kind="building"
			at="{data.parity.done} of {data.parity.done + data.parity.building + data.parity.planned} done"
			title="Everything Strapi and Sanity do"
			alt="the parity backlog: {data.parity.done} rows done, {data.parity.building} building and {data.parity.planned} planned, most of them in milestones M, E and D."
			class="lg:col-span-4"
			href="/compare/"
		>
			{#snippet graphic()}<ParityGraphic {...data.parity} />{/snippet}
			The comparison, row by row, with what was checked against their docs.
		</BentoCard>
	</div>
</section>

<!-- Real answers from the API. -->
<section aria-labelledby="api" class="shell mt-28">
	<SectionHeading id="api" eyebrow="The API" title="Answers that say what they are">
		Every read names its release; every refusal says why, with a stable code.
	</SectionHeading>
	<div class="mt-10"><ApiPanes /></div>
</section>

<!-- Real numbers, each from a committed file. -->
<section aria-labelledby="proof" class="shell mt-28">
	<SectionHeading id="proof" eyebrow="Proof, not adjectives" title="Every number comes with its file">
		All {data.resultCount} load results are committed; {data.failedCount} missed their own thresholds and are kept.
	</SectionHeading>
	<div class="mt-10"><RunsChart runs={data.runs} /></div>
	<div class="mt-4 grid gap-4 md:grid-cols-2 lg:grid-cols-6">
		<div class="lg:col-span-3"><PerfFigure figure={data.figures.site} emphasis /></div>
		<div class="lg:col-span-3"><PerfFigure figure={data.figures.delivery} emphasis /></div>
		<div class="lg:col-span-2"><PerfFigure figure={data.figures.publishing} /></div>
		<div class="lg:col-span-2"><PerfFigure figure={data.figures.entriesFresh} /></div>
		<div class="lg:col-span-2"><PerfFigure figure={data.figures.entriesLarge} /></div>
	</div>
	<p class="mt-6 max-w-3xl text-sm leading-relaxed text-muted-foreground">{LAPTOP_CAVEAT}</p>

	<dl class="mt-10 grid gap-4 sm:grid-cols-3">
		<div class="rounded-3xl border border-border/70 bg-card p-6 shadow-hairline">
			<dt class="text-sm text-muted-foreground">Planted bugs in the publishing simulator, each caught within 200 seeds</dt>
			<dd class="figure mt-2 text-4xl">8</dd>
			<dd class="mt-2 text-xs text-muted-foreground">D-047, <a class="underline underline-offset-2" href={GITHUB_FILE('docs/modules/10-sim.md')}>module 10</a></dd>
		</div>
		<div class="rounded-3xl border border-border/70 bg-card p-6 shadow-hairline">
			<dt class="text-sm text-muted-foreground">Seeds of the save simulator on every pull request, 400 steps each</dt>
			<dd class="figure mt-2 text-4xl">2,000</dd>
			<dd class="mt-2 text-xs text-muted-foreground">D-039, <a class="underline underline-offset-2" href={GITHUB_FILE('.github/workflows/ci.yml')}>CI workflow</a></dd>
		</div>
		<div class="rounded-3xl border border-border/70 bg-card p-6 shadow-hairline">
			<dt class="text-sm text-muted-foreground">Times the crash loop kills the server and worker in every CI run</dt>
			<dd class="figure mt-2 text-4xl">12</dd>
			<dd class="mt-2 text-xs text-muted-foreground">D-047, <span class="font-mono">tests/crash_loop.rs</span></dd>
		</div>
	</dl>
</section>

<!-- Open source, self-hosted. -->
<section aria-labelledby="self-host" class="shell mt-28">
	<div class="grid gap-10 rounded-[2rem] border border-border/70 bg-card p-6 shadow-soft sm:p-10 lg:grid-cols-2 lg:items-center">
		<SectionHeading id="self-host" eyebrow="Open source" title="Yours to run, read and check">
			Code, plan, decisions and results are public. Licence: Apache-2.0 proposed, not yet confirmed.
			<span class="mt-6 flex flex-wrap gap-3">
				<Button href="/open-source/">Self-hosting and licence</Button>
				<Button variant="outline" href={GITHUB}>Read the code<ArrowUpRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
			</span>
		</SectionHeading>
		<Terminal
			title="The local stack, from the README"
			lines={[
				{ cmd: 'scripts/stack.sh up' },
				{ out: '# PostgreSQL, Valkey, Garage and Keycloak on 127.0.0.1, then 28 checks' },
				{ cmd: 'scripts/migrate.sh' },
				{ cmd: 'cargo run --release -p colophon-server' },
				{ cmd: 'cargo run --release -p colophon-cli -- doctor' },
				{ out: '# exit 0 only when every dependency is ready' }
			]}
		/>
	</div>
</section>

<!-- The build log. -->
<section aria-labelledby="journal" class="shell mt-28">
	<div class="flex flex-wrap items-end justify-between gap-4">
		<SectionHeading id="journal" eyebrow="Built in the open" title="Every step, written up">
			{data.merged} steps merged{#if data.latest}, the latest on {data.latest.date}{/if}, mistakes included.
		</SectionHeading>
		<div class="flex gap-2">
			<Button variant="outline" href="/changelog/">Changelog</Button>
			<Button variant="outline" href="/blog/">All posts</Button>
		</div>
	</div>
	<ul class="mt-10 grid gap-4 md:grid-cols-3">
		{#each data.posts as post (post.slug)}
			<li>
				<a href="/blog/{post.slug}/" class="lift flex h-full flex-col rounded-3xl border border-border/70 bg-card p-6 shadow-soft outline-none focus-visible:ring-4 focus-visible:ring-ring/25">
					<p class="eyebrow">{post.milestone} · {post.step} · <time datetime={post.date}>{post.date.slice(0, 10)}</time></p>
					<h3 class="mt-3 text-[1.0625rem] leading-snug font-semibold">{post.shortTitle}</h3>
					<p class="mt-2 line-clamp-3 text-sm leading-relaxed text-muted-foreground">{post.description}</p>
					<span class="mt-auto inline-flex items-center gap-1 pt-5 text-sm font-medium">Read<ArrowRightIcon class="size-3.5" aria-hidden="true" /></span>
				</a>
			</li>
		{/each}
	</ul>
</section>

<!-- One more nudge. -->
<section aria-labelledby="start" class="shell mt-28">
	<div class="glow overflow-hidden rounded-[2rem] px-6 py-16 text-center sm:px-12">
		<div class="mx-auto max-w-2xl rounded-3xl bg-card/90 p-8 shadow-float sm:p-10">
			<h2 id="start" class="display-2">Try it on your laptop</h2>
			<p class="mt-4 text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">The CMS, the Studio and a site that rebuilds on every release, locally.</p>
			<div class="mt-8 flex flex-col justify-center gap-3 sm:flex-row">
				<Button size="lg" href={QUICKSTART} rel="external">Read the quickstart</Button>
				<Button size="lg" variant="outline" href="/roadmap/">See the roadmap</Button>
			</div>
		</div>
	</div>
</section>
