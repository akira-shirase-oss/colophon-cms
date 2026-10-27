<!--
  The reference's testimonial, without a testimonial (W3): the big quote is the question Colophon
  exists to answer, quoted from docs/WHY.md, and the cards under it are sentences copied word for
  word from the build log, each linked to its post (checked against the posts at build time, in
  +page.server.ts). The row scrolls sideways, with faded edges.
-->
<script lang="ts">
	import Mark from '#lib/components/Mark.svelte';
	import { reveal } from '#lib/motion.ts';
	import { GITHUB_FILE } from '#lib/site.ts';

	let { quotes }: { quotes: { text: string; slug: string; title: string; step: string }[] } = $props();
</script>

<section aria-labelledby="question" class="section" {@attach reveal}>
	<figure class="shell text-center">
		<blockquote>
			<h2 id="question" class="t-quote mx-auto max-w-[56.25rem]">“I published it, so why isn’t the site updated?”</h2>
		</blockquote>
		<figcaption class="mt-5 flex flex-col items-center">
			<span class="grid size-16 place-items-center rounded-full bg-card shadow-[var(--shadow-icon)]"><Mark size={26} /></span>
			<span class="mt-5 text-base leading-6 font-medium">The question Colophon answers</span>
			<a class="t-small rounded underline-offset-2 hover:underline" href={GITHUB_FILE('docs/WHY.md')}>docs/WHY.md</a>
		</figcaption>
	</figure>

	<div class="shell mt-14">
		<!-- A row wider than the page scrolls sideways; a scrollable region must be reachable by keyboard. -->
		<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
		<ul class="quotes flex snap-x gap-6 overflow-x-auto pb-2 outline-none focus-visible:ring-2 focus-visible:ring-ring" tabindex="0" aria-label="From the build log, scrollable">
			{#each quotes as quote (quote.slug)}
				<li class="glass w-[19rem] shrink-0 snap-start rounded-3xl p-8 tb:w-[24.125rem]">
					<a href="/blog/{quote.slug}/" class="flex h-full flex-col justify-between gap-8 rounded-2xl outline-none focus-visible:ring-2 focus-visible:ring-ring">
						<q class="t-body-lg block text-foreground">{quote.text}</q>
						<span class="flex items-center gap-4">
							<span class="figure grid size-14 shrink-0 place-items-center rounded-full bg-card text-[0.9375rem] font-semibold shadow-[var(--shadow-icon)]">{quote.step}</span>
							<span class="min-w-0">
								<span class="block truncate text-base leading-6 font-medium">{quote.title}</span>
								<span class="t-small block">Build log · step {quote.step}</span>
							</span>
						</span>
					</a>
				</li>
			{/each}
		</ul>
	</div>
</section>

<style>
	.quotes {
		mask-image: linear-gradient(to right, transparent 0%, #000 12.5%, #000 87.5%, transparent 100%);
		padding-inline: 12.5%;
		scroll-padding-inline: 12.5%;
		scrollbar-width: none;
	}
	.quotes::-webkit-scrollbar {
		display: none;
	}
	@media (max-width: 50.6rem) {
		.quotes {
			mask-image: none;
			padding-inline: 0;
			scroll-padding-inline: 0;
		}
	}
</style>
