<!-- A live graphic outside the bento: the dotted stage, in-view gating, and a text equivalent. -->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { inView } from '#lib/motion.ts';

	let { alt, class: className = 'h-56', interactive = false, children }: { alt: string; class?: string; interactive?: boolean; children: Snippet } =
		$props();
</script>

<figure class="overflow-hidden glass rounded-3xl">
	<div class="stage-dots relative {className}" aria-hidden={interactive ? undefined : 'true'} {@attach inView()}>
		{@render children()}
	</div>
	<figcaption class="sr-only">Illustration: {alt}</figcaption>
</figure>
