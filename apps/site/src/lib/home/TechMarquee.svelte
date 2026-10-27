<!--
  The strip under the hero, after the reference's logo ticker: a line of plain facts, then the open
  source projects Colophon is built on, as text wordmarks drifting left at 50px a second with
  faded edges. It runs only while on screen (inView); without JavaScript or with reduced motion it
  stands still.
-->
<script lang="ts">
	import { inView, reveal } from '#lib/motion.ts';

	const PARTS = ['Rust', 'Tokio', 'Axum', 'Tower', 'SQLx', 'PostgreSQL', 'Valkey', 'Garage', 'Keycloak', 'SvelteKit', 'TypeScript', 'Tailwind CSS', 'Bits UI', 'Tiptap', 'Playwright', 'k6'];

	// One copy's width over 50px a second is one loop.
	function speed(node: HTMLElement) {
		const list = node.querySelector('ul');
		if (list) node.style.setProperty('--loop', `${((list.offsetWidth + 40) / 50).toFixed(2)}s`);
	}
</script>

<section aria-labelledby="built-on" class="shell mt-20 tb:mt-24 dt:mt-[7.5rem]" {@attach reveal}>
	<h2 id="built-on" class="t-body text-center text-muted-foreground">Pre-release and built in the open, on open-source foundations</h2>
	<div class="ticker mt-4 overflow-hidden" {@attach inView(() => {}, 0.1)} {@attach speed}>
		<div class="track live flex w-max gap-10">
			{#each [0, 1] as copy (copy)}
				<ul class="flex shrink-0 gap-10" aria-hidden={copy === 1 ? 'true' : undefined}>
					{#each PARTS as part (part)}
						<li class="text-2xl leading-[55px] font-semibold tracking-[-0.03em] whitespace-nowrap text-ink-2 opacity-80">{part}</li>
					{/each}
				</ul>
			{/each}
		</div>
	</div>
</section>

<style>
	.ticker {
		mask-image: linear-gradient(90deg, transparent 5%, #000 20%, #000 80%, transparent 95%);
	}
	:global([data-run]) .track {
		animation: drift var(--loop, 40s) linear infinite;
	}
	@keyframes drift {
		to {
			transform: translateX(calc(-50% - 20px));
		}
	}
</style>
