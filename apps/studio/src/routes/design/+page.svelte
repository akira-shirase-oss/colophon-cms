<!--
  The design system (D-182, after D-056), on one page: the tokens, the type, the one status badge
  and every component in the states the Studio uses. Nothing here talks to the server, so it opens
  without signing in.
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Card from '@colophon/ui/ui/card';
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import Actions from './Actions.svelte';
	import Data from './Data.svelte';
	import Forms from './Forms.svelte';
	import Overlays from './Overlays.svelte';
	import Section from './Section.svelte';
	import Statuses from './Statuses.svelte';

	const COLOURS = [
		{ group: 'Paper and ink', tokens: ['background', 'card', 'popover', 'foreground', 'muted-foreground', 'border', 'input', 'control'] },
		{ group: 'The accent: one primary, the selection, focus', tokens: ['primary', 'primary-hover', 'brand', 'brand-ink', 'brand-soft', 'ring'] },
		{ group: 'Quiet surfaces and danger', tokens: ['secondary', 'muted', 'accent', 'destructive'] },
		{ group: 'Charts', tokens: ['chart-1', 'chart-2', 'chart-3', 'chart-4', 'chart-5'] }
	];
	const TONES = [
		{ tone: 'live', means: 'Readers have it' },
		{ tone: 'changed', means: 'Live, with changes not yet published' },
		{ tone: 'pending', means: 'On its way: scheduled, in review, being delivered' },
		{ tone: 'attention', means: 'Needs a person, or was taken off the site' },
		{ tone: 'draft', means: 'Not published' }
	];
	const TYPE = [
		{ name: 'Page title', class: 'page-title', sample: 'Storm week: how the coast road reopened' },
		{ name: 'Display', class: 'display-2', sample: 'Every release has a receipt' },
		{ name: 'Title', class: 'title', sample: 'Release 42 reached the site' },
		{ name: 'Intro', class: 'page-intro', sample: 'Group entries, get them approved, publish them together.' },
		{ name: 'Body', class: 'text-[0.9375rem] leading-relaxed', sample: 'Saved drafts are never lost, and a publish either commits whole or not at all.' },
		{ name: 'Small', class: 'text-sm text-muted-foreground', sample: 'Verified at 1 location, 4 seconds after the release.' },
		{ name: 'Figure', class: 'figure text-4xl', sample: '1,000,195' },
		{ name: 'Mono', class: 'font-mono text-sm', sample: 'sha256:9f2c…e41a' }
	];
	const RADII = [
		{ name: 'control', class: 'rounded-control', use: 'Buttons, fields' },
		{ name: 'card', class: 'rounded-card', use: 'Cards, dialogs' },
		{ name: 'full', class: 'rounded-full', use: 'Status tags' }
	];
	const SHADOWS = [
		{ name: 'flat', class: 'border border-border', use: 'Cards and buttons' },
		{ name: 'hairline', class: 'shadow-hairline', use: 'A raised segment' },
		{ name: 'float', class: 'shadow-float', use: 'Menus, dialogs' }
	];
	const NAV = ['Foundations', 'Status', 'Actions', 'Forms', 'Overlays', 'Data'];
	/** What a receipt lists: the moment this page leads with, built from the real badge. */
	const WENT_LIVE = [
		{ title: 'Storm week: how the coast road reopened', state: 'updated' },
		{ title: 'Interview: the last letterpress printer in town', state: 'new' },
		{ title: 'The Saturday market moves to the quay', state: 'taken_down' }
	] as const;
</script>

<svelte:head><title>Design system · Colophon Studio</title></svelte:head>

<div class="grid gap-10 pb-16 lg:grid-cols-[minmax(0,1fr)_minmax(0,1.05fr)] lg:items-center">
	<div class="space-y-5">
		<Badge variant="brand">Design system · D-182</Badge>
		<h1 class="display-1 max-w-xl">Quiet paper, one dusk lavender, statuses that mean something.</h1>
		<p class="page-intro max-w-lg">The Studio's tokens, type and components, taken from Colophon's public site and made calm enough to work in all day.</p>
		<nav class="flex flex-wrap gap-2" aria-label="Sections">
			{#each NAV as name (name)}
				<Button variant="outline" size="sm" href="#{name.toLowerCase()}">{name}</Button>
			{/each}
		</nav>
	</div>

	<!-- The one rich moment: a receipt, the thing only Colophon makes, drawn with the real badge. -->
	<Card.Root class="gap-5" aria-label="A release receipt, as an example">
		<Card.Header>
			<p class="text-sm text-muted-foreground">Receipt</p>
			<Card.Title class="font-display text-2xl font-[560] tracking-[-0.015em]">Release 42: Storm week</Card.Title>
			<Card.Action><StatusBadge state="release_verified" text="Verified at 2 locations" testid="hero-label" /></Card.Action>
		</Card.Header>
		<Card.Content>
			<p class="mb-2 text-sm font-medium">What went live</p>
			<ul class="divide-y divide-border overflow-hidden rounded-control border border-border bg-popover">
				{#each WENT_LIVE as item (item.title)}
					<li class="flex items-center gap-3 px-3 py-2.5 text-sm">
						<span class="min-w-0 flex-1 truncate">{item.title}</span>
						<StatusBadge state={item.state} testid="hero-{item.state}" />
					</li>
				{/each}
			</ul>
		</Card.Content>
		<Card.Footer class="flex-wrap justify-between gap-3">
			<p class="text-sm text-muted-foreground tabular">By Dev Owner, 4 seconds to the site</p>
			<Button>Publish the next one</Button>
		</Card.Footer>
	</Card.Root>
</div>

<Section id="foundations" title="Foundations" lead="Tokens in OKLCH, in styles.css. Use the token, never the value: light, dark and more contrast change underneath.">
	<div class="grid gap-10">
		{#each COLOURS as { group, tokens } (group)}
			<div>
				<p class="eyebrow mb-3">{group}</p>
				<div class="grid grid-cols-2 gap-3 sm:grid-cols-3 lg:grid-cols-4">
					{#each tokens as token (token)}
						<div class="overflow-hidden rounded-control border border-border bg-popover">
							<div class="h-14 border-b border-border" style="background: var(--{token})"></div>
							<p class="px-3 py-2 font-mono text-xs">--{token}</p>
						</div>
					{/each}
				</div>
			</div>
		{/each}

		<div>
			<p class="eyebrow mb-3">Status: a mark, a soft ring, a soft ground and an ink, each meaning one thing</p>
			<div class="divide-y divide-border overflow-hidden rounded-card border border-border bg-card">
				{#each TONES as { tone, means } (tone)}
					<div class="grid items-center gap-3 px-card py-4 sm:grid-cols-[8rem_minmax(0,1fr)_auto]">
						<p class="font-mono text-xs">--{tone}</p>
						<p class="text-sm text-muted-foreground">{means}</p>
						<div class="flex items-center gap-2" aria-hidden="true">
							{#each ['', '-ring', '-soft', '-ink'] as part (part)}
								<span class="size-7 rounded-full border border-border" style="background: var(--{tone}{part})" title="--{tone}{part}"></span>
							{/each}
						</div>
					</div>
				{/each}
			</div>
		</div>

		<div>
			<p class="eyebrow mb-3">Type: DM Sans for page titles and display, Inter for everything people read and use</p>
			<div class="divide-y divide-border overflow-hidden rounded-card border border-border bg-card">
				{#each TYPE as item (item.name)}
					<div class="grid gap-2 px-card py-5 sm:grid-cols-[9rem_minmax(0,1fr)] sm:items-baseline">
						<p class="text-xs text-muted-foreground">{item.name}</p>
						<p class={item.class}>{item.sample}</p>
					</div>
				{/each}
			</div>
		</div>

		<div class="grid gap-10 md:grid-cols-2">
			<div>
				<p class="eyebrow mb-3">Shape: <span class="font-mono">--radius-control</span> and <span class="font-mono">--radius-card</span></p>
				<div class="flex flex-wrap gap-3">
					{#each RADII as radius (radius.name)}
						<div class="grid h-20 w-28 place-items-center border border-border bg-card text-center text-sm {radius.class}">
							<span>{radius.name}<span class="block text-xs text-muted-foreground">{radius.use}</span></span>
						</div>
					{/each}
				</div>
			</div>
			<div>
				<p class="eyebrow mb-3">Elevation: only what floats casts a shadow</p>
				<div class="flex flex-wrap gap-4">
					{#each SHADOWS as shadow (shadow.name)}
						<div class="grid h-20 w-28 place-items-center rounded-card bg-popover text-center text-sm {shadow.class}">
							<span>{shadow.name}<span class="block text-xs text-muted-foreground">{shadow.use}</span></span>
						</div>
					{/each}
				</div>
			</div>
		</div>

		<div>
			<p class="eyebrow mb-3">A card: <span class="font-mono">--card-padding</span> inside, <span class="font-mono">--radius-card</span> outside, a hairline, no shadow</p>
			<div class="grid gap-4 md:grid-cols-2">
				<Card.Root>
					<Card.Header>
						<Card.Title>Your drafts</Card.Title>
						<Card.Description>What you started that readers don't have yet.</Card.Description>
					</Card.Header>
					<Card.Content class="text-sm text-muted-foreground">Nothing half-finished.</Card.Content>
				</Card.Root>
				<Card.Root size="sm">
					<Card.Header>
						<Card.Title>Compact</Card.Title>
						<Card.Description><span class="font-mono">--card-padding-sm</span>, for dense panels.</Card.Description>
					</Card.Header>
					<Card.Content class="text-sm text-muted-foreground">Text lines up with the title.</Card.Content>
				</Card.Root>
			</div>
		</div>
	</div>
</Section>

<Section id="status" title="Status" lead="One StatusBadge for every entry, request, release and schedule: an icon and a word on its tone's ground, never colour alone.">
	<Statuses />
</Section>

<Section id="actions" title="Actions" lead="One primary per screen, in the accent. Everything else is secondary, ghost, or, for a confirmation's last step, destructive.">
	<Actions />
</Section>

<Section id="forms" title="Forms" lead="White fields on a hairline, the accent on focus and on what is chosen, errors said in words beside the field.">
	<Forms />
</Section>

<Section id="overlays" title="Overlays" lead="Dialogs keep to the screen: the body scrolls, the footer stays, and fields stack label, help, input.">
	<Overlays />
</Section>

<Section id="data" title="Data and layout" lead="Tables, lists, cards and navigation: hairline rules, tabular numbers, and room to breathe.">
	<Data />
</Section>

<p class="pt-6 text-sm text-muted-foreground">
	Every component lives in <code class="font-mono">packages/ui/src/lib/components/ui</code>.
	<a class="inline-flex items-center gap-0.5 font-medium text-brand-ink underline underline-offset-4" href="https://shadcn-svelte.com/docs/components">
		shadcn-svelte docs<ArrowUpRightIcon class="size-3.5" />
	</a>
</p>
