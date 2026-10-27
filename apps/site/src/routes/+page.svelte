<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import PerfFigure from '#lib/components/PerfFigure.svelte';
	import SectionHeading from '#lib/components/SectionHeading.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import StatusBadge from '#lib/components/StatusBadge.svelte';
	import { DESCRIPTION, GITHUB, GITHUB_FILE, LAPTOP_CAVEAT, QUICKSTART } from '#lib/site.ts';
	import CrashCard from '#lib/visuals/CrashCard.svelte';
	import EditorCard from '#lib/visuals/EditorCard.svelte';
	import LabelStates from '#lib/visuals/LabelStates.svelte';
	import PublishRequestCard from '#lib/visuals/PublishRequestCard.svelte';
	import ReceiptCard from '#lib/visuals/ReceiptCard.svelte';
	import ReleaseSlider from '#lib/visuals/ReleaseSlider.svelte';
	import SeedCard from '#lib/visuals/SeedCard.svelte';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const STEPS = [
		{
			n: '1',
			title: 'Write',
			text: 'Drafts save as you type. Every save is a new version that names the one it was made on, so two people never overwrite each other.',
			under: 'Insert-only versions, If-Match saves, idempotency keys'
		},
		{
			n: '2',
			title: 'Review',
			text: 'Put entries in a publish request. The reviewer approves that exact set of versions; edit one afterwards and it needs approving again.',
			under: 'Publish requests pinned by hash (D-044)'
		},
		{
			n: '3',
			title: 'Publish',
			text: 'Publishing is one numbered release, committed whole or not at all. Release numbers only go up, so nothing can put an older one back.',
			under: 'Release generations, a transactional outbox (D-045, D-046)'
		},
		{
			n: '4',
			title: 'Verified',
			text: 'Colophon looks for the release on the live site and says where it saw it: verified at N locations, or still pending.',
			under: 'Probes and stored evidence (D-051)'
		}
	];
</script>

<Seo title="Colophon: the open-source CMS that can prove what it published" description={DESCRIPTION} />

<!-- The promise, in one sentence, over the product it describes. -->
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
			Colophon is an open-source content management system in Rust and Svelte. Every publish becomes one numbered release, with a receipt
			that says where it was seen, and nothing you saved is lost when a process dies halfway through.
		</p>
		<div class="rise mt-8 flex flex-col items-center justify-center gap-3 sm:flex-row" style="--i: 3">
			<Button size="lg" href={QUICKSTART} rel="external">Read the quickstart<ArrowRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
			<Button size="lg" variant="outline" href={GITHUB}>View on GitHub<ArrowUpRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
		</div>
		<p class="mt-5 text-sm text-muted-foreground">Open source · self-hosted · PostgreSQL is the only source of truth</p>
	</div>

	<div class="shell">
		<div class="glow relative rounded-[2rem] px-4 py-10 sm:px-10 sm:py-14">
			<div class="mx-auto grid max-w-5xl items-center gap-6 lg:grid-cols-[minmax(0,1fr)_minmax(0,25rem)_minmax(0,1fr)]">
				<EditorCard class="hidden -rotate-2 lg:block lg:translate-x-6" />
				<ReceiptCard class="relative z-10" />
				<PublishRequestCard class="hidden rotate-2 lg:block lg:-translate-x-6 lg:translate-y-10" />
			</div>
		</div>
		<p class="mt-3 text-center text-xs text-muted-foreground">Illustrations built from the Studio’s real states and words, not data from a real site.</p>
	</div>
</section>

<!-- The four promises, each with how far along it is. -->
<section aria-labelledby="promises" class="shell mt-28">
	<SectionHeading id="promises" eyebrow="The four promises" title="Built around what is hard to fake">
		Each promise has a test that tries to break it, and a label that says how far along it is.
	</SectionHeading>

	<div class="mt-12 grid gap-4 lg:grid-cols-6">
		<article class="flex flex-col gap-6 rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-8 lg:col-span-4 lg:flex-row">
			<div class="flex-1 space-y-3">
				<StatusBadge kind="done" at="P1, P2" />
				<h3 class="title">Kill it at any moment, lose nothing</h3>
				<p class="text-[0.9375rem] leading-relaxed text-muted-foreground">
					No saved edit disappears, no release runs twice, and no reader sees half of one, even when the server or the worker dies in the
					middle of a publish.
				</p>
				<p class="text-sm"><span class="font-medium">Proven by</span> <span class="text-muted-foreground">fault tests against the real binary (F01 to F05) and a crash loop that kills the server and worker twelve times in every CI run.</span></p>
			</div>
			<CrashCard class="content-start lg:w-[19rem]" />
		</article>

		<article class="flex flex-col gap-5 rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-8 lg:col-span-2">
			<StatusBadge kind="done" at="P3" />
			<h3 class="title">Receipts, not spinners</h3>
			<p class="text-[0.9375rem] leading-relaxed text-muted-foreground">
				“Published” only means published. The site update shows as pending until a probe sees the release on the live page.
			</p>
			<LabelStates class="mt-auto" />
		</article>

		<article class="flex flex-col gap-5 rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-8 lg:col-span-3">
			<StatusBadge kind="done" at="P1, P2" />
			<h3 class="title">Every bug is a number you can replay</h3>
			<p class="text-[0.9375rem] leading-relaxed text-muted-foreground">
				A deterministic simulator runs thousands of seeded schedules of colliding editors, crashed workers and a network that drops and
				duplicates. A failure comes with a seed that reproduces it exactly. Delivery and cache faults join it in step 3.8.
			</p>
			<SeedCard class="mt-auto" />
		</article>

		<article class="flex flex-col gap-5 rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-8 lg:col-span-3">
			<div class="flex flex-wrap gap-2"><StatusBadge kind="planned" at="P4" /><Badge variant="outline">Groundwork done in P2</Badge></div>
			<h3 class="title">Time travel</h3>
			<p class="text-[0.9375rem] leading-relaxed text-muted-foreground">
				Browse the whole site as it was at any release; rolling back is just a new release. Every version is already kept, and the API can
				already answer as of any past release; the slider and rollback come in P4.
			</p>
			<ReleaseSlider class="mt-auto" />
		</article>
	</div>
</section>

<!-- Write, review, publish, verified. -->
<section aria-labelledby="how" class="mt-28 border-y border-border bg-card/60 py-20">
	<div class="shell">
		<SectionHeading id="how" eyebrow="How it works" title="One path from a draft to a page you can prove is live">
			The Studio, the command line, scripts and agents all go through the same API and the same permission checks.
		</SectionHeading>
		<ol class="mt-12 grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
			{#each STEPS as step (step.n)}
				<li class="relative flex flex-col rounded-3xl border border-border/70 bg-card p-6 shadow-hairline">
					<span class="figure grid size-9 place-items-center rounded-full bg-primary text-sm text-primary-foreground" aria-hidden="true">{step.n}</span>
					<h3 class="title mt-5">{step.title}</h3>
					<p class="mt-2 text-[0.9375rem] leading-relaxed text-muted-foreground">{step.text}</p>
					<p class="mt-auto pt-5 text-xs text-muted-foreground"><span class="font-medium text-foreground">Underneath:</span> {step.under}</p>
				</li>
			{/each}
		</ol>
	</div>
</section>

<!-- Real numbers, each from a committed file. -->
<section aria-labelledby="proof" class="shell mt-28">
	<SectionHeading id="proof" eyebrow="Proof, not adjectives" title="Every number comes with its file">
		Every feature ships with regression tests, a page in a separate test app, and a load test of at least a million requests. All {data.resultCount}
		load results are committed, failures included: {data.failedCount} missed their own thresholds, and runs slower than the machine’s baseline allows
		are kept too.
	</SectionHeading>

	<div class="mt-12 grid gap-4 md:grid-cols-2 lg:grid-cols-6">
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

<!-- Strapi and Sanity, honestly. -->
<section aria-labelledby="parity" class="shell mt-28">
	<div class="grid gap-10 lg:grid-cols-[minmax(0,0.9fr)_minmax(0,1.1fr)] lg:items-start">
		<SectionHeading id="parity" eyebrow="Strapi and Sanity" title="Every feature they have, at this bar of proof">
			The goal since 27 October 2026 (D-057): what Strapi and Sanity offer, built so it can be proven. Some of it is done, most of it is planned,
			and the comparison says which, row by row.
			<span class="mt-6 block">
				<Button variant="outline" href="/compare/">See the full comparison<ArrowRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
			</span>
		</SectionHeading>
		<div class="overflow-hidden rounded-3xl border border-border/70 bg-card shadow-soft">
			<table class="w-full text-sm">
				<caption class="sr-only">A few rows of the parity backlog</caption>
				<thead class="border-b border-border bg-muted/50 text-xs text-muted-foreground">
					<tr>
						<th scope="col" class="px-4 py-3 text-left font-medium">Feature</th>
						<th scope="col" class="hidden px-3 py-3 text-left font-medium sm:table-cell">Strapi</th>
						<th scope="col" class="hidden px-3 py-3 text-left font-medium sm:table-cell">Sanity</th>
						<th scope="col" class="px-4 py-3 text-left font-medium">Colophon</th>
					</tr>
				</thead>
				<tbody class="divide-y divide-border">
					{#each data.parity as row (row.feature)}
						<tr>
							<th scope="row" class="px-4 py-3 text-left font-medium">{row.feature}</th>
							<td class="hidden px-3 py-3 text-muted-foreground sm:table-cell">{row.strapi}</td>
							<td class="hidden px-3 py-3 text-muted-foreground sm:table-cell">{row.sanity}</td>
							<td class="px-4 py-3"><StatusBadge kind={row.kind} /><span class="mt-1 block text-xs text-muted-foreground">{row.colophon}</span></td>
						</tr>
					{/each}
				</tbody>
			</table>
		</div>
	</div>
</section>

<!-- Open source, self-hosted. -->
<section aria-labelledby="self-host" class="shell mt-28">
	<div class="grid gap-10 rounded-[2rem] border border-border/70 bg-card p-6 shadow-soft sm:p-10 lg:grid-cols-2 lg:items-center">
		<SectionHeading id="self-host" eyebrow="Open source" title="Yours to run, read and check">
			The code, the plan, every decision and every load result are public. Colophon runs on your own machines with PostgreSQL, a disposable
			cache, S3-compatible storage and any OpenID Connect sign-in. The licence is proposed as Apache-2.0 and not yet confirmed.
			<span class="mt-6 flex flex-wrap gap-3">
				<Button href="/open-source/">Self-hosting and licence</Button>
				<Button variant="outline" href={GITHUB}>Read the code<ArrowUpRightIcon data-icon="inline-end" aria-hidden="true" /></Button>
			</span>
		</SectionHeading>
		<div class="overflow-hidden rounded-2xl border border-border bg-background font-mono text-[0.8125rem] leading-relaxed">
			<p class="border-b border-border px-4 py-2.5 font-sans text-xs text-muted-foreground">The local stack, from the README</p>
			<div class="space-y-1 overflow-x-auto px-4 py-4">
				<p><span class="text-muted-foreground">$</span> scripts/stack.sh up</p>
				<p class="text-muted-foreground"># PostgreSQL, Valkey, Garage and Keycloak on 127.0.0.1, then 28 checks</p>
				<p class="pt-2"><span class="text-muted-foreground">$</span> scripts/migrate.sh</p>
				<p><span class="text-muted-foreground">$</span> cargo run --release -p colophon-server</p>
				<p class="pt-2"><span class="text-muted-foreground">$</span> cargo run --release -p colophon-cli -- doctor</p>
				<p class="text-muted-foreground"># exit 0 only when every dependency is ready</p>
			</div>
		</div>
	</div>
</section>

<!-- The build log. -->
<section aria-labelledby="journal" class="shell mt-28">
	<div class="flex flex-wrap items-end justify-between gap-4">
		<SectionHeading id="journal" eyebrow="Built in the open" title="Every step, written up">
			{data.merged} steps merged so far{#if data.latest}, the latest on {data.latest.date}{/if}. The build log tells each one, mistakes included.
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
			<p class="mt-4 text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">
				Colophon is pre-release, so it runs locally today: the CMS, the Studio and a static site that rebuilds on every release.
			</p>
			<div class="mt-8 flex flex-col justify-center gap-3 sm:flex-row">
				<Button size="lg" href={QUICKSTART} rel="external">Read the quickstart</Button>
				<Button size="lg" variant="outline" href="/roadmap/">See the roadmap</Button>
			</div>
		</div>
	</div>
</section>
