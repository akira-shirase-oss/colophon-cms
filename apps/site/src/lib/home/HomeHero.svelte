<!--
  The home page's first screen, after the reference's hero: a sky-to-sand band, a two-line
  headline whose words arrive one by one out of a blur, a lead, two pills, and the product in a
  framed window that lies back in perspective and stands up as the page scrolls (perspective(1000px)
  translateY(-80px) scale(0.8) rotateX(20deg) at its start, flat after 700px of scrolling; on load
  it eases from its start to where the scroll puts it). Without JavaScript or with reduced motion
  the words are simply there and the window is flat.
-->
<script lang="ts">
	import ArrowUpRightIcon from '@lucide/svelte/icons/arrow-up-right';
	import Pill from '#lib/components/Pill.svelte';
	import { easeOutQuart, reducedMotion, scrollLinked } from '#lib/motion.ts';
	import { GITHUB, QUICKSTART } from '#lib/site.ts';
	import HeroStory from './HeroStory.svelte';

	const TITLE = 'A CMS that can prove what it published';
	const LEAD = 'Open source and pre-release, in Rust and Svelte. Every publish becomes one numbered release, with a receipt that says where it was seen.';
	const titleWords = TITLE.split(' ');
	const leadWords = LEAD.split(' ');

	// The window's tilt: 1 lies fully back, 0 is flat. The scroll sets a target; for the first
	// 1.2s after load the tilt eases from fully back to that target.
	let tilt = $state<number | null>(null);
	let target = 1;
	let started = 0;
	let raf = 0;
	function settle() {
		const intro = easeOutQuart((performance.now() - started) / 1200);
		tilt = 1 - intro * (1 - target);
		raf = intro < 1 ? requestAnimationFrame(settle) : 0;
	}
	const follow = scrollLinked(
		(progress) => {
			target = 1 - progress;
			if (!started) {
				started = performance.now();
				raf = requestAnimationFrame(settle);
			} else if (!raf) tilt = target;
		},
		{ start: 1, distance: 700, offset: 20 }
	);
	$effect(() => {
		// No IntersectionObserver, no scroll link: stand the window up rather than leave it tilted.
		if (!reducedMotion() && !('IntersectionObserver' in window)) tilt = 0;
		return () => cancelAnimationFrame(raf);
	});
	const moving = $derived(tilt !== null && !reducedMotion());
</script>

<section aria-labelledby="hero-title" class="hero hero-ground relative overflow-hidden">
	<div class="cloud cloud-left" aria-hidden="true"></div>
	<div class="cloud cloud-right" aria-hidden="true"></div>
	<div class="shell relative pt-32 text-center max-tb:px-5 tb:pt-[9.5rem] dt:pt-40">
		<h1 id="hero-title" class="t-h1 mx-auto max-w-[49.5rem]">
			{#each titleWords as word, index (index)}<span class="word-in inline-block" style="--w: {index}">{word}</span>{index < titleWords.length - 1 ? ' ' : ''}{/each}
		</h1>
		<p class="t-lead mx-auto mt-4 max-w-[49.5rem]">
			{#each leadWords as word, index (index)}<span class="word-in inline-block" style="--w: {index}; --w0: 100ms">{word}</span>{index < leadWords.length - 1 ? ' ' : ''}{/each}
		</p>
		<div class="fade-in mt-10 flex flex-col justify-center gap-2 tb:flex-row" style="--fade-delay: 740ms">
			<Pill label="Read the quickstart" href={QUICKSTART} external />
			<Pill label="View on GitHub" href={GITHUB} variant="tertiary">
				{#snippet icon()}<ArrowUpRightIcon class="size-4" aria-hidden="true" />{/snippet}
			</Pill>
		</div>
	</div>

	<div class="shell relative mt-14 tb:mt-16 dt:mt-20" {@attach follow}>
		<div class="window-size mx-auto max-tb:mx-0">
			<div class="window frame-line rounded-[20px]" style={moving ? `--tilt: ${tilt?.toFixed(4)}` : undefined}>
				<div class="window-scale"><HeroStory /></div>
			</div>
		</div>
	</div>
	<p class="sr-only">The picture above is an illustration built from the Studio’s real states and words, not data from a real site.</p>
</section>

<style>
	/* 1,020px tall on a desktop and 800 below, like the reference; the window runs off its bottom. */
	.hero {
		height: 800px;
	}
	@media (min-width: 75rem) {
		.hero {
			height: 1020px;
		}
	}

	/* The window: 1,028 by 620 on a desktop; 714 by 476 below, its contents drawn at desktop size
	   and scaled, as the reference does. */
	.window-size {
		width: 714px;
		height: 476px;
	}
	.window-scale {
		width: 1028px;
		height: 685px;
		transform: scale(0.6946);
		transform-origin: 0 0;
	}
	@media (min-width: 75rem) {
		.window-size {
			width: 1028px;
			height: 620px;
		}
		.window-scale {
			height: 620px;
			transform: none;
		}
	}
	.window {
		position: relative;
		width: 100%;
		height: 100%;
		overflow: hidden;
		transform: perspective(1000px) translateY(calc(var(--tilt, 0) * -80px)) scale(calc(1 - var(--tilt, 0) * 0.2)) rotateX(calc(var(--tilt, 0) * 20deg));
		transform-origin: 50% 50%;
		will-change: transform;
	}
	/* With JavaScript and motion allowed, the window starts lying back, so hydration doesn't snap it. */
	@media (prefers-reduced-motion: no-preference) {
		:global(html.js) .window {
			--tilt: 1;
		}
	}

	/* Soft clouds at the band's edges: our own shapes, drawn with gradients. */
	.cloud {
		position: absolute;
		top: 180px;
		width: 480px;
		height: 262px;
		pointer-events: none;
		background:
			radial-gradient(closest-side, var(--cloud) 55%, transparent) 22% 62% / 44% 62% no-repeat,
			radial-gradient(closest-side, var(--cloud) 55%, transparent) 50% 40% / 46% 78% no-repeat,
			radial-gradient(closest-side, var(--cloud) 55%, transparent) 76% 58% / 40% 60% no-repeat,
			radial-gradient(closest-side, var(--cloud) 60%, transparent) 50% 82% / 92% 38% no-repeat;
		filter: blur(5px);
	}
	.cloud-left {
		left: -280px;
	}
	.cloud-right {
		right: -280px;
		transform: scaleX(-1);
	}
	@media (max-width: 74.99rem) {
		.cloud {
			top: 200px;
			width: 240px;
			height: 131px;
		}
		.cloud-left {
			left: -160px;
		}
		.cloud-right {
			right: -160px;
		}
	}
</style>
