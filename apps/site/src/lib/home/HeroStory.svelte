<!--
  The hero's product visual, drawn as the Studio's window (its sidebar's real sections and words)
  inside the reference's dashboard frame, playing a short story of what Colophon does: a draft is typed and saved as
  version 7, a publish request is approved for exactly those versions and published as release 42,
  and the receipt checks off Saved, Published in CMS, Website update pending and Verified at 1
  location, ending on the evidence it kept. Then it rests; Replay plays it again.

  The prerendered HTML is the final frame, which is what readers without JavaScript or with reduced
  motion get. With JavaScript, `html.js` (app.html) holds the first frame until the story starts.
  Everything that moves is opacity or transform; the title is typed by changing its text inside a
  fixed-size field. The picture is decorative (aria-hidden); the sentence after it says the same.
-->
<script lang="ts">
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import CheckIcon from '@lucide/svelte/icons/check';
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import Mark from '#lib/components/Mark.svelte';
	import { inView, reducedMotion } from '#lib/motion.ts';

	const TITLE = 'Spring menu';
	const ROWS = [
		{ title: 'Spring menu', version: 7 },
		{ title: 'Opening hours', version: 3 },
		{ title: 'About us', version: 12 }
	];
	// The beats, in order; Final is the resting frame.
	const Beat = { Idle: 0, Typing: 1, Saved: 2, Rows: 3, Approved: 4, Pressed: 5, Published: 6, Pending: 7, Verified: 8, Final: 9 } as const;
	type Beat = (typeof Beat)[keyof typeof Beat];
	// When each beat starts, in milliseconds from the start: about six seconds in all.
	const SCHEDULE: [number, Beat][] = [
		[250, Beat.Typing],
		[1450, Beat.Saved],
		[2050, Beat.Rows],
		[2900, Beat.Approved],
		[3300, Beat.Pressed],
		[3600, Beat.Published],
		[4000, Beat.Pending],
		[5700, Beat.Verified],
		[6150, Beat.Final]
	];
	const KEY_MS = 85;

	let beat = $state<Beat>(Beat.Final);
	let typed = $state(TITLE.length);
	let started = $state(false);
	let timers: ReturnType<typeof setTimeout>[] = [];

	function clear() {
		for (const timer of timers) clearTimeout(timer);
		timers = [];
	}

	function play() {
		clear();
		started = true;
		if (reducedMotion()) {
			beat = Beat.Final;
			typed = TITLE.length;
			return;
		}
		beat = Beat.Idle;
		typed = 0;
		for (const [at, next] of SCHEDULE) timers.push(setTimeout(() => (beat = next), at));
		for (let index = 1; index <= TITLE.length; index++) {
			timers.push(setTimeout(() => (typed = index), SCHEDULE[0]![0] + 120 + index * KEY_MS));
		}
	}

	$effect(() => clear);

	const at = (from: Beat) => beat >= from;
	type Step = 'todo' | 'active' | 'done';
	const steps = $derived<{ label: string; detail: string; state: Step }[]>([
		{ label: 'Saved', detail: 'Version 7 of “Spring menu”, autosaved', state: at(Beat.Saved) ? 'done' : 'todo' },
		{ label: 'Published in CMS', detail: 'Release 42, one transaction', state: at(Beat.Published) ? 'done' : 'todo' },
		{
			label: 'Website update pending',
			detail: 'Built from release 42, swapped in at once',
			state: at(Beat.Verified) ? 'done' : at(Beat.Pending) ? 'active' : 'todo'
		},
		{ label: 'Verified at 1 location', detail: 'The probe saw release 42 on the live page', state: at(Beat.Verified) ? 'done' : 'todo' }
	]);
	const LABELS = [
		{ text: 'Not published yet', tone: 'draft' },
		{ text: 'Published in CMS', tone: 'draft' },
		{ text: 'Website update pending', tone: 'pending' },
		{ text: 'Verified at 1 location', tone: 'live' }
	] as const;
	const label = $derived(
		at(Beat.Verified)
			? { text: 'Verified at 1 location', tone: 'live' }
			: at(Beat.Pending)
				? { text: 'Website update pending', tone: 'pending' }
				: at(Beat.Published)
					? { text: 'Published in CMS', tone: 'draft' }
					: { text: 'Not published yet', tone: 'draft' }
	);
</script>

<div
	class="story relative isolate h-full w-full overflow-hidden rounded-[20px] bg-card text-left"
	data-story
	data-phase={beat === Beat.Final ? 'final' : beat}
	data-started={started ? '' : undefined}
	{@attach inView((state) => {
		if (state === 'on' && !started) play();
	}, 0.2)}
>
	<div aria-hidden="true" class="app grid h-full grid-cols-[11rem_minmax(0,1fr)]">
		<!-- The Studio's sidebar, with its real sections and words. -->
		<div class="sidebar flex flex-col gap-1 border-r border-border px-3 py-4">
			<div class="flex items-center gap-2 px-1 pb-3">
				<span class="grid size-7 place-items-center rounded-lg border border-border bg-card"><Mark size={13} /></span>
				<span class="min-w-0 leading-tight">
					<span class="block truncate text-[0.8125rem] font-semibold">Demo project</span>
					<span class="block truncate text-[0.6875rem] text-muted-foreground">demo</span>
				</span>
			</div>
			<span class="side-item">Overview</span>
			<span class="side-item">Search<kbd class="ml-auto rounded border border-border px-1 font-sans text-[0.625rem] text-muted-foreground">⌘K</kbd></span>
			<span class="side-label">Content</span>
			<span class="side-item active">Articles<span class="ml-auto text-[0.6875rem] text-muted-foreground tabular">3</span></span>
			<span class="side-label">Publishing</span>
			<span class="side-item">Changesets<span class="ml-auto rounded-full bg-pending-soft px-1.5 text-[0.625rem] text-pending-ink tabular">1</span></span>
			<span class="side-item">Releases<span class="ml-auto text-[0.6875rem] text-muted-foreground tabular">No. 42</span></span>
			<span class="side-label">Project</span>
			<span class="side-item">Members</span>
			<span class="side-item">Settings</span>
		</div>

		<div class="flex min-w-0 flex-col">
			<!-- The top bar: where you are, and the title search. -->
			<div class="flex h-14 items-center gap-3 border-b border-border px-5">
				<span class="text-[0.8125rem] text-muted-foreground">Articles <span class="px-1">/</span> <span class="text-foreground">Spring menu</span></span>
				<span class="ml-auto flex h-8 w-48 items-center gap-2 rounded-full border border-border px-3 text-[0.75rem] text-muted-foreground">Search titles<kbd class="ml-auto font-sans text-[0.625rem]">⌘K</kbd></span>
				<span class="grid size-8 place-items-center rounded-full bg-primary text-[0.6875rem] font-semibold text-primary-foreground">ED</span>
			</div>

			<div class="grid flex-1 grid-cols-[minmax(0,1fr)_minmax(0,1.18fr)_minmax(0,1fr)] items-start gap-4 bg-background/60 p-4">
				<!-- The draft: the title is typed, then the save lands. -->
				<div class="rounded-2xl border border-border bg-card p-4">
					<div class="flex items-center justify-between gap-2">
						<p class="text-[0.75rem] font-medium text-muted-foreground">Articles · Draft</p>
						<span class="beat pop inline-flex items-center gap-1 rounded-full bg-live-soft px-2 py-0.5 text-[0.6875rem] font-medium whitespace-nowrap text-live-ink" data-on={at(Beat.Saved)}>
							<CheckIcon class="size-3" />Saved · version 7
						</span>
					</div>
					<p class="mt-4 text-[0.6875rem] font-medium text-muted-foreground">Title</p>
					<div class="mt-1 flex h-9 items-center rounded-xl border bg-card px-3 text-[0.8125rem] font-medium {beat === Beat.Typing ? 'border-ring' : 'border-input'}">
						<span class="title-text whitespace-pre">{TITLE.slice(0, typed)}</span><span class="caret" data-on={beat <= Beat.Typing}></span>
					</div>
					<p class="mt-3 text-[0.6875rem] font-medium text-muted-foreground">Body</p>
					<div class="mt-1 space-y-2 rounded-xl border border-input bg-card px-3 py-3">
						<span class="block h-2 w-11/12 rounded-full bg-muted"></span>
						<span class="block h-2 w-10/12 rounded-full bg-muted"></span>
						<span class="block h-2 w-7/12 rounded-full bg-muted"></span>
						<span class="block h-2 w-9/12 rounded-full bg-muted"></span>
					</div>
					<p class="mt-3 text-[0.75rem] leading-snug text-muted-foreground">A copy of unsaved edits stays on this device.</p>
				</div>

				<!-- The receipt: four steps, checked off as they happen. -->
				<div class="rounded-2xl border border-border bg-card p-4">
					<div class="flex items-center gap-3">
						<span class="figure grid size-9 place-items-center rounded-xl bg-brand-soft text-[0.875rem] font-semibold text-brand-ink">42</span>
						<div class="min-w-0">
							<p class="text-[0.8125rem] font-semibold">Release 42</p>
							<p class="text-[0.6875rem] text-muted-foreground">production · 3 entries</p>
						</div>
					</div>
					<!-- The release's one label; all four are stacked in one place, so changing it moves nothing. -->
					<div class="labels mt-3 grid justify-items-start">
						{#each LABELS as candidate (candidate.text)}
							<span
								class="label beat pop inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.6875rem] font-medium whitespace-nowrap"
								data-tone={candidate.tone}
								data-on={label.text === candidate.text}
							>
								{#if candidate.tone === 'live'}<BadgeCheckIcon class="size-3" />{:else if candidate.tone === 'pending'}<ClockIcon class="size-3" />{:else}<CircleDashedIcon
										class="size-3"
									/>{/if}{candidate.text}
							</span>
						{/each}
					</div>
					<ol class="mt-4 grid">
						{#each steps as step, index (step.label)}
							<li class="relative grid grid-cols-[1.5rem_minmax(0,1fr)] gap-2.5 pb-3 last:pb-0" data-step={step.state}>
								{#if index < steps.length - 1}
									<span class="absolute top-6 bottom-0 left-[0.6875rem] w-px bg-border"></span>
									<span class="rail absolute top-6 bottom-0 left-[0.6875rem] w-px origin-top bg-live"></span>
								{/if}
								<span class="dot relative grid size-6 place-items-center rounded-full border">
									<span class="tick"><CheckIcon class="size-3" strokeWidth={2.5} /></span>
									<span class="wait absolute size-2 rounded-full bg-pending"></span>
								</span>
								<div class="step-text">
									<p class="text-[0.8125rem] font-medium">{step.label}</p>
									<p class="text-[0.6875rem] leading-snug text-muted-foreground">{step.detail}</p>
								</div>
							</li>
						{/each}
					</ol>
					<div class="beat rise-in mt-4 flex flex-wrap items-center justify-between gap-1 border-t border-border pt-3 text-[0.6875rem] text-muted-foreground" data-on={at(Beat.Final)}>
						<span>Evidence kept: release, location, time, body hash</span>
						<code class="font-mono">sha256:9f2c…e41a</code>
					</div>
				</div>

				<!-- The publish request: rows light up, the approval lands, Publish is pressed. -->
				<div class="rounded-2xl border border-border bg-card p-4">
					<div class="flex items-center justify-between gap-2">
						<p class="text-[0.8125rem] font-semibold">Publish request</p>
						<span class="beat pop inline-flex items-center gap-1 rounded-full bg-live-soft px-2 py-0.5 text-[0.6875rem] font-medium text-live-ink" data-on={at(Beat.Approved)}>
							<ShieldCheckIcon class="size-3" />Approved
						</span>
					</div>
					<ul class="mt-3 divide-y divide-border overflow-hidden rounded-xl border border-border">
						{#each ROWS as row, index (row.title)}
							<li class="row relative flex items-center justify-between gap-2 px-3 py-2 text-[0.8125rem]" style="--i: {index}" data-on={at(Beat.Rows)}>
								<span class="truncate">{row.title}</span>
								<span class="figure shrink-0 text-[0.6875rem] text-muted-foreground">version {row.version}</span>
							</li>
						{/each}
					</ul>
					<p class="mt-3 text-[0.75rem] leading-snug text-muted-foreground">Approved for these exact versions. Edit one, and it needs approving again.</p>
					<div class="mt-4 flex items-center justify-between gap-2">
						<code class="font-mono text-[0.6875rem] text-muted-foreground">hash 4be1…07c2</code>
						<span class="press inline-flex h-8 items-center rounded-full bg-primary px-4 text-[0.8125rem] font-semibold text-primary-foreground" data-on={beat === Beat.Pressed}>
							Publish
						</span>
					</div>
				</div>
			</div>
		</div>
	</div>

	<p class="sr-only">
		Illustration: a draft titled Spring menu is autosaved as version 7, put in a publish request that is approved for those exact versions, and
		published as release 42. Its receipt then moves from Published in CMS to Website update pending to Verified at 1 location, and keeps the
		evidence: the release, the location, the time and a hash of the page.
	</p>
	<button
		type="button"
		class="replay absolute right-4 bottom-4 z-20 inline-flex h-9 items-center gap-1.5 rounded-full bg-primary px-4 text-[0.8125rem] font-semibold text-primary-foreground outline-none hover:bg-primary/85 focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-offset-2"
		data-on={started && beat === Beat.Final}
		onclick={play}
	>
		<RotateCcwIcon class="size-3.5" aria-hidden="true" />Replay<span class="sr-only"> the illustration</span>
	</button>
</div>

<style>
	/* The resting frame is the default; the rules below only describe how each piece arrives. */
	.beat,
	.row,
	.press,
	.caret,
	.dot,
	.tick,
	.wait,
	.rail,
	.step-text,
	.replay {
		transition:
			opacity 320ms cubic-bezier(0.2, 0.8, 0.2, 1),
			transform 320ms cubic-bezier(0.2, 0.8, 0.2, 1),
			background-color 320ms ease,
			border-color 320ms ease,
			color 320ms ease;
	}
	.beat[data-on='false'] {
		opacity: 0;
	}
	.pop[data-on='false'] {
		transform: scale(0.85);
	}
	.rise-in[data-on='false'] {
		transform: translateY(6px);
	}

	/* The release's one label, in its state's colours; a new label pops in. */
	.label[data-tone='live'] {
		background: var(--live-soft);
		color: var(--live-ink);
	}
	.label[data-tone='pending'] {
		background: var(--pending-soft);
		color: var(--pending-ink);
	}
	.label[data-tone='draft'] {
		background: var(--draft-soft);
		color: var(--draft-ink);
	}
	.labels > * {
		grid-area: 1 / 1;
	}

	/* The caret blinks while the title is typed. */
	.caret {
		display: inline-block;
		width: 1.5px;
		height: 1.1em;
		margin-left: 1px;
		background: var(--foreground);
		opacity: 0;
	}
	.story[data-started] .caret[data-on='true'] {
		animation: blink 0.9s steps(1) infinite;
	}
	@keyframes blink {
		0%,
		50% {
			opacity: 1;
		}
		50.01%,
		100% {
			opacity: 0;
		}
	}

	/* Rows light up one after another while the reviewer reads them. */
	.row::before {
		content: '';
		position: absolute;
		inset: 0;
		background: var(--brand-soft);
		opacity: 0;
		z-index: -1;
	}
	.story[data-started] .row[data-on='true']::before {
		animation: row-read 900ms ease both;
		animation-delay: calc(var(--i) * 260ms);
	}
	@keyframes row-read {
		35% {
			opacity: 1;
		}
		to {
			opacity: 0;
		}
	}
	.row {
		isolation: isolate;
	}

	/* Publish is pressed. */
	.press[data-on='true'] {
		transform: scale(0.94);
	}

	/* Receipt steps: a grey ring until done; done is a green tick; the pending one breathes. */
	.dot {
		border-color: var(--border);
		background: var(--card);
		color: var(--live-ink);
	}
	.tick {
		display: grid;
		opacity: 1;
	}
	.wait {
		opacity: 0;
	}
	.rail {
		transform: scaleY(1);
		opacity: 0.6;
	}
	[data-step='todo'] .tick,
	[data-step='active'] .tick {
		opacity: 0;
		transform: scale(0.6);
	}
	[data-step='todo'] .rail,
	[data-step='active'] .rail {
		transform: scaleY(0);
	}
	/* A step not reached yet reads in the caption grey, which still passes AA, rather than faded. */
	[data-step='todo'] .step-text {
		color: var(--muted-foreground);
	}
	[data-step='active'] .dot {
		border-color: var(--pending);
	}
	[data-step='active'] .wait {
		opacity: 1;
		animation: breathe 1.6s ease-in-out infinite;
	}
	[data-step='done']:last-child .dot {
		background: var(--live);
		border-color: var(--live);
		color: white;
	}
	@keyframes breathe {
		50% {
			opacity: 0.3;
		}
	}

	/* Replay appears once the story rests. */
	.replay[data-on='false'] {
		opacity: 0;
		pointer-events: none;
	}

	/* Before the story starts (JavaScript on, motion allowed): the first frame, so the end never flashes. */
	@media (prefers-reduced-motion: no-preference) {
		:global(html.js) .story:not([data-started]) :is(.beat, .replay) {
			opacity: 0;
		}
		:global(html.js) .story:not([data-started]) .title-text {
			opacity: 0;
		}
		:global(html.js) .story:not([data-started]) :is(.tick, .rail) {
			opacity: 0;
		}
		:global(html.js) .story:not([data-started]) .step-text {
			color: var(--muted-foreground);
		}
		:global(html.js) .story:not([data-started]) li:last-child .dot {
			background: var(--card);
			border-color: var(--border);
		}
	}

	/* The sidebar of the Studio, in miniature. */
	.side-item {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		border-radius: 0.5rem;
		padding: 0.375rem 0.5rem;
		font-size: 0.8125rem;
		color: var(--ink-2);
	}
	.side-item.active {
		background: var(--accent);
		color: var(--foreground);
		font-weight: 500;
	}
	.side-label {
		margin-top: 0.625rem;
		padding: 0.25rem 0.5rem;
		font-size: 0.6875rem;
		font-weight: 500;
		color: var(--muted-foreground);
	}
	.sidebar {
		background: var(--background);
	}

	@media (prefers-reduced-motion: reduce) {
		.story *,
		.story *::before {
			animation: none !important;
			transition: none !important;
		}
	}
</style>
