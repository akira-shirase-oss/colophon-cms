<!--
  The hero's product visual as a short story of what Colophon does: a draft is typed and saved as
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
	import { inView, pointerVars, reducedMotion } from '#lib/motion.ts';

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
	class="story glow relative isolate overflow-hidden rounded-[2rem] px-4 py-8 sm:px-8 sm:py-12 lg:py-14"
	data-story
	data-phase={beat === Beat.Final ? 'final' : beat}
	data-started={started ? '' : undefined}
	{@attach pointerVars}
	{@attach inView((state) => {
		if (state === 'on' && !started) play();
	}, 0.2)}
>
	<div aria-hidden="true" class="relative z-10 mx-auto grid max-w-5xl items-center gap-4 sm:grid-cols-2 sm:gap-5 lg:grid-cols-[minmax(0,1fr)_minmax(0,25rem)_minmax(0,1fr)] lg:gap-6">
		<!-- The draft: the title is typed, then the save lands. -->
		<div class="tilt rounded-3xl border border-border/60 bg-card p-5 text-left shadow-float sm:col-start-1 lg:rotate-[-2deg] lg:translate-x-6" style="--depth: 10">
			<div class="flex items-center justify-between gap-3">
				<p class="eyebrow">Articles · Draft</p>
				<span class="beat pop inline-flex items-center gap-1.5 rounded-full bg-live-soft px-2.5 py-0.5 text-xs font-medium text-live-ink" data-on={at(Beat.Saved)}>
					<CheckIcon class="size-3" />Saved · version 7
				</span>
			</div>
			<p class="mt-4 text-[0.6875rem] font-medium text-muted-foreground">Title</p>
			<div class="mt-1 flex h-10 items-center rounded-xl border bg-card px-3 text-sm font-medium shadow-hairline {beat === Beat.Typing ? 'border-ring' : 'border-input'}">
				<span class="title-text whitespace-pre">{TITLE.slice(0, typed)}</span><span class="caret" data-on={beat <= Beat.Typing}></span>
			</div>
			<p class="mt-3 hidden text-[0.6875rem] font-medium text-muted-foreground sm:block">Body</p>
			<div class="mt-1 hidden space-y-2 rounded-xl border border-input bg-card px-3 py-3 shadow-hairline sm:block">
				<span class="block h-2 w-11/12 rounded-full bg-muted"></span>
				<span class="block h-2 w-10/12 rounded-full bg-muted"></span>
				<span class="block h-2 w-7/12 rounded-full bg-muted"></span>
			</div>
			<p class="mt-3 text-xs text-muted-foreground">A copy of unsaved edits stays on this device.</p>
		</div>

		<!-- The receipt: four steps, checked off as they happen. -->
		<div class="tilt rounded-3xl border border-border/60 bg-card p-5 text-left shadow-float sm:row-span-2 sm:col-start-2 sm:row-start-1 sm:p-6 lg:row-span-1" style="--depth: 4">
			<div class="flex items-center gap-3">
				<span class="figure grid size-10 place-items-center rounded-xl bg-brand-soft text-[0.9375rem] font-semibold text-brand-ink">42</span>
				<div class="min-w-0">
					<p class="text-sm font-semibold">Release 42</p>
					<p class="text-xs text-muted-foreground">production · 3 entries</p>
				</div>
			</div>
			<!-- The release's one label; all four are stacked in one place, so changing it moves nothing. -->
			<div class="labels mt-4 grid justify-items-start">
				{#each LABELS as candidate (candidate.text)}
					<span
						class="label beat pop inline-flex items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium whitespace-nowrap"
						data-tone={candidate.tone}
						data-on={label.text === candidate.text}
					>
						{#if candidate.tone === 'live'}<BadgeCheckIcon class="size-3" />{:else if candidate.tone === 'pending'}<ClockIcon class="size-3" />{:else}<CircleDashedIcon
								class="size-3"
							/>{/if}{candidate.text}
					</span>
				{/each}
			</div>
			<ol class="mt-5 grid">
				{#each steps as step, index (step.label)}
					<li class="relative grid grid-cols-[1.75rem_minmax(0,1fr)] gap-3 pb-4 last:pb-0" data-step={step.state}>
						{#if index < steps.length - 1}
							<span class="absolute top-7 bottom-0 left-[0.8125rem] w-px bg-border"></span>
							<span class="rail absolute top-7 bottom-0 left-[0.8125rem] w-px origin-top bg-live"></span>
						{/if}
						<span class="dot relative grid size-7 place-items-center rounded-full border">
							<span class="tick"><CheckIcon class="size-3.5" strokeWidth={2.5} /></span>
							<span class="wait absolute size-2 rounded-full bg-pending"></span>
						</span>
						<div class="step-text pt-0.5">
							<p class="text-sm font-medium">{step.label}</p>
							<p class="text-xs text-muted-foreground">{step.detail}</p>
						</div>
					</li>
				{/each}
			</ol>
			<div class="beat rise-in mt-5 flex flex-wrap items-center justify-between gap-2 border-t border-border pt-4 text-xs text-muted-foreground" data-on={at(Beat.Final)}>
				<span>Evidence kept: release, location, time, body hash</span>
				<code class="font-mono">sha256:9f2c…e41a</code>
			</div>
		</div>

		<!-- The publish request: rows light up, the approval lands, Publish is pressed. -->
		<div class="tilt hidden rounded-3xl border border-border/60 bg-card p-5 text-left shadow-float sm:col-start-1 sm:block lg:col-start-3 lg:row-start-1 lg:translate-y-10 lg:-translate-x-6 lg:rotate-2" style="--depth: 14">
			<div class="flex items-center justify-between gap-3">
				<p class="text-sm font-semibold">Publish request</p>
				<span class="beat pop inline-flex items-center gap-1.5 rounded-full bg-live-soft px-2.5 py-0.5 text-xs font-medium text-live-ink" data-on={at(Beat.Approved)}>
					<ShieldCheckIcon class="size-3" />Approved
				</span>
			</div>
			<ul class="mt-4 divide-y divide-border overflow-hidden rounded-2xl border border-border">
				{#each ROWS as row, index (row.title)}
					<li class="row relative flex items-center justify-between gap-3 px-3 py-2 text-sm" style="--i: {index}" data-on={at(Beat.Rows)}>
						<span class="truncate">{row.title}</span>
						<span class="figure shrink-0 text-xs text-muted-foreground">version {row.version}</span>
					</li>
				{/each}
			</ul>
			<p class="mt-3 text-xs text-muted-foreground">Approved for these exact versions. Edit one, and it needs approving again.</p>
			<div class="mt-4 flex items-center justify-between gap-3">
				<code class="font-mono text-[0.6875rem] text-muted-foreground">hash 4be1…07c2</code>
				<span class="press inline-flex h-9 items-center rounded-full bg-primary px-4 text-sm font-medium text-primary-foreground shadow-press" data-on={beat === Beat.Pressed}>
					Publish
				</span>
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
		class="replay absolute right-3 bottom-3 z-20 inline-flex h-8 items-center gap-1.5 rounded-full border border-border/70 bg-card/90 px-3 text-xs font-medium shadow-soft outline-none hover:bg-card focus-visible:ring-4 focus-visible:ring-ring/25 sm:right-5 sm:bottom-5"
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
	[data-step='todo'] .step-text {
		opacity: 0.45;
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
			opacity: 0.45;
		}
		:global(html.js) .story:not([data-started]) li:last-child .dot {
			background: var(--card);
			border-color: var(--border);
		}
	}

	/* Depth: the cards lean toward the pointer and lift on hover; a soft light follows it. */
	@media (hover: hover) and (pointer: fine) and (prefers-reduced-motion: no-preference) {
		.tilt {
			transform: perspective(1100px) rotateX(calc(var(--py, 0) * -3deg)) rotateY(calc(var(--px, 0) * 4deg))
				translate3d(calc(var(--px, 0) * var(--depth) * 1px), calc(var(--py, 0) * var(--depth) * 1px + var(--lift, 0px)), 0);
			transition:
				transform 500ms cubic-bezier(0.2, 0.8, 0.2, 1),
				box-shadow 300ms ease;
			will-change: transform;
		}
		.tilt:hover {
			--lift: -6px;
		}
		.story::after {
			content: '';
			position: absolute;
			inset: 0;
			z-index: 0;
			pointer-events: none;
			background: radial-gradient(320px circle at var(--mx, 50%) var(--my, 50%), oklch(1 0 0 / 0.38), transparent 70%);
			opacity: 0;
			transition: opacity 400ms ease;
		}
		.story:global([data-pointer])::after {
			opacity: 1;
		}
		:global(.dark) .story::after {
			background: radial-gradient(320px circle at var(--mx, 50%) var(--my, 50%), oklch(0.75 0.12 50 / 0.22), transparent 70%);
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.story *,
		.story *::before {
			animation: none !important;
			transition: none !important;
		}
	}
</style>
