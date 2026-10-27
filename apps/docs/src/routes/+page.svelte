<!--
  The front page: what Colophon is, that it is pre-release, the four promises with what is built
  of each, and the way into every section. No numbers here: measured figures live on the load
  testing page, each with its machine.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import CapabilityLabel from '#lib/components/CapabilityLabel.svelte';
	import { type Capability, DESIGN, editUrl, HOME, SECTIONS } from '#lib/nav.ts';

	const LABELS = [
		{ text: 'Published in CMS', variant: 'draft', icon: CircleDashedIcon, meaning: 'The release committed; nothing has checked the website yet.' },
		{ text: 'Website update pending', variant: 'pending', icon: ClockIcon, meaning: 'Its webhooks, purges or checks are still under way.' },
		{ text: 'Verified at 1 location', variant: 'live', icon: BadgeCheckIcon, meaning: 'A probe saw this release on the site, from one place.' },
		{ text: 'Needs attention', variant: 'attention', icon: TriangleAlertIcon, meaning: 'Something gave up, and a person should look.' }
	] as const;

	const PROMISES: { title: string; body: string; capability: Capability; href: string }[] = [
		{
			title: 'Kill it at any moment, lose nothing',
			body: 'Saves name the version they build on, every write carries an idempotency key, and a release is one transaction. Crash tests kill the real server before and after commits, and a crash loop kills it at random.',
			capability: { kind: 'implemented' },
			href: '/concepts/saves-and-conflicts/'
		},
		{
			title: 'Every bug is a number you can replay',
			body: 'A deterministic simulator runs thousands of seeded schedules of colliding editors, crashing workers and a lossy network on every push. A failure prints the seed that replays it.',
			capability: { kind: 'implemented' },
			href: '/concepts/simulator/'
		},
		{
			title: 'Receipts, not spinners',
			body: 'Each release ends with one label. "Verified" always says at how many locations a probe saw it, and a purge the CDN accepted is never reported as copies gone.',
			capability: { kind: 'implemented' },
			href: '/concepts/receipts-and-probes/'
		},
		{
			title: 'Time travel',
			body: 'Reads pinned to any past release already work through the delivery API. The release slider in the Studio and rollback as a new release come with milestone P4.',
			capability: { kind: 'planned', milestone: 'P4' },
			href: '/concepts/releases-and-generations/'
		}
	];

	const CARDS = [
		...SECTIONS.map((section) => ({
			title: section.title,
			href: section.href,
			lead: section.pages[0]?.description ?? '',
			links: section.pages.slice(1, 4)
		})),
		{ title: DESIGN.title, href: DESIGN.href, lead: 'The plan itself, rendered from docs/: the end goal, architecture, system design, decisions and every module.', links: [] }
	];
</script>

<svelte:head>
	<title>Colophon documentation</title>
	<meta name="description" content={HOME.description} />
</svelte:head>

<div class="page-enter" data-pagefind-body>
	<section class="grid items-center gap-10 pb-14 lg:grid-cols-[minmax(0,1.1fr)_minmax(0,1fr)] lg:gap-14 lg:pb-20">
		<div class="space-y-6">
			<div class="flex flex-wrap items-center gap-2" data-pagefind-ignore>
				<Badge variant="brand">Colophon documentation</Badge>
				<CapabilityLabel capability={HOME.capability} />
			</div>
			<h1 class="display-1 max-w-2xl" data-pagefind-meta="title">A CMS that can prove what it published.</h1>
			<p class="max-w-xl text-[1.0625rem] leading-relaxed text-pretty text-muted-foreground">
				Colophon is an open-source content management system in Rust and Svelte. Every publish becomes one numbered release, and every release ends with
				a receipt of where it was actually seen. It is <strong class="font-semibold text-foreground">pre-release</strong>: v0.1 is not out, milestone P3 is
				in progress, and nothing here is ready for production.
			</p>
			<div class="flex flex-wrap gap-3">
				<Button href="/get-started/quickstart/" size="lg">Run it locally <ArrowRightIcon data-icon="inline-end" /></Button>
				<Button href="/editors/" variant="outline" size="lg">Read the editor guide</Button>
			</div>
		</div>

		<div class="glow relative overflow-hidden rounded-[2rem] p-5 sm:p-8" data-pagefind-ignore>
			<div class="relative mx-auto max-w-md rounded-3xl border border-border bg-card p-6 shadow-float">
				<p class="title">Every release ends with one label</p>
				<p class="mt-1 text-sm text-muted-foreground">The words the Studio shows, from the release's stored evidence.</p>
				<ul class="mt-5 space-y-3">
					{#each LABELS as label (label.text)}
						<li class="flex flex-col gap-1.5 rounded-2xl border border-border bg-background p-3 sm:flex-row sm:items-center sm:gap-3">
							<Badge variant={label.variant} class="shrink-0"><label.icon aria-hidden="true" />{label.text}</Badge>
							<span class="text-[0.8125rem] leading-snug text-muted-foreground">{label.meaning}</span>
						</li>
					{/each}
				</ul>
			</div>
		</div>
	</section>

	<section aria-labelledby="promises" class="border-t border-border py-14">
		<div class="mb-8 max-w-2xl space-y-2">
			<h2 id="promises" class="display-2">Four promises, and how much of each is built</h2>
			<p class="text-[0.9375rem] text-pretty text-muted-foreground">
				A feature counts only when its failure behaviour is tested and its evidence is stored. Each card says where it stands today.
			</p>
		</div>
		<ul class="grid gap-4 md:grid-cols-2">
			{#each PROMISES as promise (promise.title)}
				<li class="flex flex-col rounded-3xl border border-border bg-card p-6 shadow-soft">
					<div class="mb-3"><CapabilityLabel capability={promise.capability} /></div>
					<h3 class="title">{promise.title}</h3>
					<p class="mt-2 flex-1 text-sm leading-relaxed text-muted-foreground">{promise.body}</p>
					<a
						href={promise.href}
						class="mt-4 inline-flex w-fit items-center gap-1.5 rounded-md text-sm font-medium underline decoration-brand underline-offset-4 outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
						>How it works<ArrowRightIcon class="size-3.5" aria-hidden="true" /></a
					>
				</li>
			{/each}
		</ul>
	</section>

	<section aria-labelledby="sections" class="border-t border-border py-14">
		<h2 id="sections" class="display-2 mb-8">Find your way</h2>
		<ul class="grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
			{#each CARDS as card (card.href)}
				<li class="flex flex-col rounded-3xl border border-border bg-card p-6 shadow-hairline">
					<h3 class="title"><a href={card.href} class="rounded-md outline-none hover:underline focus-visible:ring-[3px] focus-visible:ring-ring/40">{card.title}</a></h3>
					<p class="mt-2 text-sm leading-relaxed text-muted-foreground">{card.lead}</p>
					{#if card.links.length > 0}
						<ul class="mt-4 space-y-1.5 text-sm">
							{#each card.links as link (link.href)}
								<li><a href={link.href} class="underline decoration-border underline-offset-4 hover:decoration-foreground">{link.title}</a></li>
							{/each}
						</ul>
					{/if}
				</li>
			{/each}
		</ul>
	</section>

	<section aria-labelledby="labels" class="border-t border-border py-14" data-pagefind-ignore>
		<div class="grid gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
			<div class="space-y-2">
				<h2 id="labels" class="display-2">How to read the labels</h2>
				<p class="text-[0.9375rem] text-pretty text-muted-foreground">Every page carries one, and parts of a page that differ carry their own. When in doubt, a page says planned.</p>
			</div>
			<dl class="divide-y divide-border overflow-hidden rounded-3xl border border-border bg-card shadow-hairline">
				<div class="grid gap-2 p-5 sm:grid-cols-[11rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={{ kind: 'implemented' }} /></dt>
					<dd class="text-sm text-muted-foreground">Built, with tests, in the code this site was built from (the commit in the footer).</dd>
				</div>
				<div class="grid gap-2 p-5 sm:grid-cols-[11rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={{ kind: 'building', step: 'step' }} /></dt>
					<dd class="text-sm text-muted-foreground">In progress on its own branch, named by its step. Not in this code yet.</dd>
				</div>
				<div class="grid gap-2 p-5 sm:grid-cols-[11rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={{ kind: 'planned', milestone: 'P4' }} /></dt>
					<dd class="text-sm text-muted-foreground">Designed, not built, with the milestone it belongs to. Milestones are estimates, not promises.</dd>
				</div>
				<div class="grid gap-2 p-5 sm:grid-cols-[11rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={{ kind: 'design' }} /></dt>
					<dd class="text-sm text-muted-foreground">A design document from <code class="font-mono text-[0.8125rem]">docs/</code>: the plan, mixing what is built with what is not.</dd>
				</div>
			</dl>
		</div>
		<p class="mt-10 text-sm text-muted-foreground">
			<a href={editUrl(HOME.source)} rel="external" class="underline decoration-border underline-offset-4 hover:decoration-foreground">Edit this page on GitHub</a>
		</p>
	</section>
</div>
