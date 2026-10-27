<!--
  A build-log post's cover, in place of the reference's photographs: the step's number set large
  on a soft ground, a ground picked by the step. Decorative: the card around it names the post.
-->
<script lang="ts">
	import Mark from './Mark.svelte';
	import { MILESTONE_NAMES } from '#lib/blog.ts';

	let { step, milestone, large = false }: { step: string; milestone: string; large?: boolean } = $props();
	// Neighbouring posts get different grounds: the step's last digit picks one.
	const TONES = ['sky', 'sand', 'dusk', 'sea'];
	const tone = $derived(TONES[(Number(step.replace(/\D/g, '').slice(-1)) || 0) % TONES.length]);
</script>

<div class="cover relative h-full w-full overflow-hidden" data-tone={tone} aria-hidden="true">
	<div class="ground absolute inset-0 flex flex-col justify-between p-6 {large ? 'tb:p-8' : ''}">
		<span class="flex items-center gap-2 text-[0.8125rem] font-medium"><Mark size={14} />{milestone} · {MILESTONE_NAMES[milestone] ?? milestone}</span>
		<span class="step font-semibold tracking-[-0.05em] {large ? 'text-[7rem] tb:text-[9rem]' : 'text-[5.5rem]'} leading-none">{step}</span>
	</div>
</div>

<style>
	.cover {
		color: #1a1615;
	}
	.ground {
		transition: transform 600ms cubic-bezier(0.22, 1, 0.36, 1);
	}
	:global(a:hover) .ground {
		transform: scale(1.145);
	}
	[data-tone='sky'] .ground {
		background: linear-gradient(160deg, #a9c9ea 0%, #d6e3ef 55%, #ecdfd1 100%);
	}
	[data-tone='sand'] .ground {
		background: linear-gradient(160deg, #f1e6da 0%, #e6d3c1 60%, #d8c0aa 100%);
	}
	[data-tone='dusk'] .ground {
		background: linear-gradient(160deg, #cdbfdf 0%, #e7d6dc 55%, #f1dfd2 100%);
	}
	[data-tone='sea'] .ground {
		background: linear-gradient(160deg, #a9d3cc 0%, #cfe3dc 55%, #e8e4d5 100%);
	}
	:global(.dark) .cover {
		color: #f4f1ee;
	}
	/* Dark keeps to warm browns: umber, sand, rust and clay. */
	:global(.dark) [data-tone='sky'] .ground {
		background: linear-gradient(160deg, #3a2c22 0%, #2e241d 55%, #251e19 100%);
	}
	:global(.dark) [data-tone='sand'] .ground {
		background: linear-gradient(160deg, #3d3226 0%, #322920 60%, #28211b 100%);
	}
	:global(.dark) [data-tone='dusk'] .ground {
		background: linear-gradient(160deg, #432a20 0%, #35241c 55%, #2a1f19 100%);
	}
	:global(.dark) [data-tone='sea'] .ground {
		background: linear-gradient(160deg, #3b3324 0%, #2f291e 55%, #26211a 100%);
	}
</style>
