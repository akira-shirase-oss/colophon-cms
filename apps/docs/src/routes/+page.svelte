<!--
  The docs' front page, in the website's look: what Colophon is and that it is pre-release, the way
  into every section, the promises with where each stands, what changed lately (from CHANGELOG.md)
  and how to read the statuses. No numbers here: measured figures live on the performance page,
  each with its machine.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import BookOpenIcon from '@lucide/svelte/icons/book-open';
	import BracesIcon from '@lucide/svelte/icons/braces';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import LightbulbIcon from '@lucide/svelte/icons/lightbulb';
	import PenLineIcon from '@lucide/svelte/icons/pen-line';
	import RocketIcon from '@lucide/svelte/icons/rocket';
	import ServerIcon from '@lucide/svelte/icons/server';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import CapabilityLabel from '#lib/components/CapabilityLabel.svelte';
	import { building, type Capability, done, editUrl, HOME, planned } from '#lib/nav.ts';
	import { href, site } from '#lib/paths.ts';
	import { STATUS_MEANING } from '#lib/status.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();

	const CARDS = [
		{
			icon: RocketIcon,
			title: 'Get started',
			href: '/get-started/',
			lead: 'Run the whole stack on your laptop, make a project, write an article and publish it with a receipt.',
			links: [
				{ href: '/get-started/quickstart/', title: 'Run it locally' },
				{ href: '/get-started/first-publish/', title: 'Your first publish and receipt' }
			]
		},
		{
			icon: PenLineIcon,
			title: 'For editors',
			href: '/editors/',
			lead: 'Writing, versions, publish requests, publishing and receipts, in plain words.',
			links: [
				{ href: '/editors/review/', title: 'Publish requests and review' },
				{ href: '/editors/receipts/', title: 'Receipts: is it on the website?' }
			]
		},
		{
			icon: LightbulbIcon,
			title: 'Concepts',
			href: '/concepts/',
			lead: 'How Colophon keeps its promises: versions, releases, the outbox, caching, receipts and roles.',
			links: [
				{ href: '/concepts/releases-and-generations/', title: 'Releases and generations' },
				{ href: '/concepts/roles-and-permissions/', title: 'Roles and permissions' }
			]
		},
		{
			icon: BracesIcon,
			title: 'Developer reference',
			href: '/reference/',
			lead: 'Every HTTP route, header and error code, the delivery API, the CLI, rich text and webhooks.',
			links: [
				{ href: '/reference/api/routes/', title: 'All routes' },
				{ href: '/reference/api/errors/', title: 'Errors' }
			]
		},
		{
			icon: ServerIcon,
			title: 'Operations',
			href: '/operations/',
			lead: 'What it needs, every setting, health checks, the worker and every migration.',
			links: [
				{ href: '/operations/configuration/', title: 'Configuration' },
				{ href: '/operations/upgrades/', title: 'Upgrades and migrations' }
			]
		},
		{
			icon: HistoryIcon,
			title: 'Updates',
			href: '/changelog/',
			lead: 'The changelog with its feed, release notes per milestone and upgrade guides.',
			links: [
				{ href: '/releases/', title: 'Release notes' },
				{ href: '/upgrading/', title: 'Upgrade guides' }
			]
		}
	];

	const PROMISES: { title: string; body: string; capability: Capability; href: string }[] = [
		{
			title: 'Kill it at any moment, lose nothing',
			body: 'Saves name the version they build on, every write carries an idempotency key, and a release is one transaction. Crash tests kill the real server before and after commits, and a crash loop kills it at random.',
			capability: done('P2'),
			href: '/concepts/saves-and-conflicts/'
		},
		{
			title: 'Receipts, not spinners',
			body: 'Each release ends with one label. "Verified" always says at how many locations a probe saw it, and a purge the CDN accepted is never reported as copies gone.',
			capability: done('3.3'),
			href: '/concepts/receipts-and-probes/'
		},
		{
			title: 'Roles you define, enforced everywhere',
			body: 'Four default roles, roles owners define, and permissions down to a field or to your own entries, all checked by one deny-by-default evaluator. API tokens and an audit log come next.',
			capability: done('R.3'),
			href: '/concepts/roles-and-permissions/'
		},
		{
			title: 'Time travel',
			body: 'Reads pinned to any past release already work through the delivery API. The release slider in the Studio and rollback as a new release come with milestone P4.',
			capability: planned('P4'),
			href: '/concepts/releases-and-generations/'
		}
	];

	const LONG_DATE = new Intl.DateTimeFormat('en-GB', { day: 'numeric', month: 'short', year: 'numeric', timeZone: 'UTC' });
</script>

<svelte:head>
	<title>Colophon documentation</title>
	<meta name="description" content={HOME.description} />
</svelte:head>

<div class="page-enter" data-pagefind-body>
	<section class="hero relative overflow-hidden rounded-[32px] px-6 pt-14 pb-12 text-center tb:px-10 tb:pt-20 tb:pb-16">
		<p class="chip" data-pagefind-ignore>Documentation</p>
		<h1 class="t-h2 mx-auto mt-4 max-w-[44rem]" data-pagefind-meta="title">A CMS that can prove what it published</h1>
		<p class="t-lead mx-auto mt-4 max-w-[40rem] text-pretty">
			Colophon is an open-source content management system in Rust and Svelte. Every publish becomes one numbered release, and every release ends with a receipt of
			where it was actually seen. It is <strong class="font-semibold text-foreground">pre-release</strong>: v0.1 is not out, milestone P3 is in progress, and nothing here
			is for production use.
		</p>
		<div class="mt-8 flex flex-col justify-center gap-2 tb:flex-row" data-pagefind-ignore>
			<a href={href('/get-started/quickstart/')} class="pill pill-primary pill-small"><span class="roll"><span>Run it locally</span><span aria-hidden="true">Run it locally</span></span></a>
			<a href={href('/editors/')} class="pill pill-outline pill-small"><span class="roll"><span>Read the editor guide</span><span aria-hidden="true">Read the editor guide</span></span></a>
		</div>
		<p class="t-small mt-6" data-pagefind-ignore>
			New here? Read <a class="text-link" href={href('/get-started/')}>what Colophon is</a> or the
			<a class="text-link" href={site('/')} rel="external">website</a>.
		</p>
	</section>

	<section aria-labelledby="sections" class="mt-16 tb:mt-20">
		<h2 id="sections" class="t-h5">Find your way</h2>
		<ul class="mt-6 grid gap-4 sm:grid-cols-2 xl:grid-cols-3">
			{#each CARDS as card (card.href)}
				<li class="card flex flex-col rounded-[24px] p-6">
					<span class="icon grid size-10 place-items-center rounded-full" aria-hidden="true"><card.icon class="size-[18px]" /></span>
					<h3 class="t-h6 mt-4">
						<a href={href(card.href)} class="rounded-md outline-none hover:underline focus-visible:ring-2 focus-visible:ring-ring">{card.title}</a>
					</h3>
					<p class="t-body mt-2 flex-1 text-[0.9375rem]">{card.lead}</p>
					<ul class="mt-4 space-y-1.5 text-[0.9375rem]">
						{#each card.links as link (link.href)}
							<li>
								<a href={href(link.href)} class="text-link inline-flex items-center gap-1">{link.title}<ArrowRightIcon class="size-3.5" aria-hidden="true" /></a>
							</li>
						{/each}
					</ul>
				</li>
			{/each}
		</ul>
	</section>

	<section aria-labelledby="latest" class="mt-16 grid gap-6 tb:mt-20 lg:grid-cols-[minmax(0,1fr)_minmax(0,1fr)]">
		<div class="card rounded-[24px] p-6 tb:p-8">
			<div class="flex flex-wrap items-baseline justify-between gap-2">
				<h2 id="latest" class="t-h5">What changed lately</h2>
				<a href={href('/changelog/')} class="text-link text-[0.9375rem]">The changelog</a>
			</div>
			<ol class="mt-5 grid gap-3">
				{#each data.latest as entry (entry.id)}
					<li class="flex flex-wrap items-baseline gap-x-3 gap-y-1">
						<span class="font-mono text-[0.8125rem] text-muted-foreground">{entry.date ? LONG_DATE.format(new Date(`${entry.date}T00:00:00Z`)) : ''}</span>
						<a href={href(`/changelog/#${entry.id}`)} class="text-link">{entry.step} · {entry.title}</a>
					</li>
				{/each}
			</ol>
		</div>
		<div class="card rounded-[24px] p-6 tb:p-8">
			<h2 class="t-h5">Being built now</h2>
			<p class="t-small mt-2">On branches, not merged: none of this is in the code yet.</p>
			<ul class="mt-5 grid gap-3">
				{#each data.building as entry (entry.id)}
					<li class="flex flex-wrap items-center gap-x-3 gap-y-1">
						<CapabilityLabel capability={{ kind: 'building', at: entry.step }} size="small" />
						<a href={href(`/changelog/#${entry.id}`)} class="text-link">{entry.title}</a>
					</li>
				{/each}
			</ul>
		</div>
	</section>

	<section aria-labelledby="promises" class="mt-16 tb:mt-20">
		<h2 id="promises" class="t-h5">The promises, and how much of each is built</h2>
		<p class="t-body mt-2 max-w-[44rem]">A feature counts only when its failure behaviour is tested and its evidence is stored. Each card says where it stands today.</p>
		<ul class="mt-6 grid gap-4 md:grid-cols-2">
			{#each PROMISES as promise (promise.title)}
				<li class="card flex flex-col rounded-[24px] p-6">
					<div><CapabilityLabel capability={promise.capability} /></div>
					<h3 class="t-h6 mt-4">{promise.title}</h3>
					<p class="t-body mt-2 flex-1 text-[0.9375rem]">{promise.body}</p>
					<a href={href(promise.href)} class="text-link mt-4 inline-flex w-fit items-center gap-1 text-[0.9375rem]">How it works<ArrowRightIcon class="size-3.5" aria-hidden="true" /></a>
				</li>
			{/each}
		</ul>
	</section>

	<section aria-labelledby="labels" class="mt-16 tb:mt-20" data-pagefind-ignore>
		<div class="grid gap-8 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.4fr)]">
			<div>
				<h2 id="labels" class="t-h5">How to read the statuses</h2>
				<p class="t-body mt-2">The website's three words, on every page and every part of a page that differs. When in doubt, a page says planned.</p>
				<p class="t-small mt-4 flex items-center gap-2"><ShieldCheckIcon class="size-4" aria-hidden="true" />The footer names the commit these statuses describe.</p>
			</div>
			<dl class="card divide-y divide-[color:var(--line)] overflow-hidden rounded-[24px]">
				<div class="grid gap-2 p-5 sm:grid-cols-[10rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={done('3.5')} /></dt>
					<dd class="t-body text-[0.9375rem]">{STATUS_MEANING.done} The step that built it.</dd>
				</div>
				<div class="grid gap-2 p-5 sm:grid-cols-[10rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={building('site/docs')} /></dt>
					<dd class="t-body text-[0.9375rem]">{STATUS_MEANING.building} The step, or the branch when there is no step yet.</dd>
				</div>
				<div class="grid gap-2 p-5 sm:grid-cols-[10rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={planned('P4')} /></dt>
					<dd class="t-body text-[0.9375rem]">{STATUS_MEANING.planned} Milestones are estimates, not promises.</dd>
				</div>
				<div class="grid gap-2 p-5 sm:grid-cols-[10rem_minmax(0,1fr)]">
					<dt><CapabilityLabel capability={{ kind: 'design' }} /></dt>
					<dd class="t-body text-[0.9375rem]">A design document from <code class="font-mono text-[0.8125rem]">docs/</code>: the plan, mixing what is built with what is not.</dd>
				</div>
			</dl>
		</div>
		<p class="t-small mt-10">
			<a href={editUrl(HOME.source)} rel="external" class="text-link">Edit this page on GitHub</a>
		</p>
	</section>
</div>

<style>
	/* The website's sky-to-sand band (warm browns in dark), from the shared token. */
	.hero {
		background: var(--hero-ground);
	}
	.card {
		background: var(--card);
		box-shadow: inset 0 0 0 1px var(--line);
	}
	.icon {
		background: var(--primary);
		color: var(--primary-foreground);
		box-shadow: var(--shadow-icon);
	}
</style>
