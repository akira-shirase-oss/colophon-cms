<!--
  A line that lights up once, word by word, the website's closing statement (D-210 addendum, Studio v2): readable at rest
  in `rest` (never hidden), each word brightening to `on` in reading order, about 700 ms in all, a colour
  transition only. It plays when it is first on screen (it is on screen at load on the Overview); with reduced
  motion it is simply `on`. One IntersectionObserver for this line only, disconnected once it fires.
-->
<script lang="ts">
	import { onMount } from 'svelte';

	let { text, rest = 'var(--muted-foreground)', on = 'var(--foreground)', class: className = '' }: { text: string; rest?: string; on?: string; class?: string } = $props();

	const words = $derived(text.split(/(\s+)/).filter(Boolean));
	const count = $derived(words.filter((word) => !/^\s+$/.test(word)).length);
	let line: HTMLElement | undefined = $state();
	let phase = $state<'rest' | 'go' | undefined>(undefined);

	onMount(() => {
		if (!line || matchMedia('(prefers-reduced-motion: reduce)').matches || !('IntersectionObserver' in window)) return;
		phase = 'rest';
		const observer = new IntersectionObserver((seen) => {
			if (seen[0]?.isIntersecting) {
				observer.disconnect();
				requestAnimationFrame(() => requestAnimationFrame(() => (phase = 'go')));
			}
		});
		observer.observe(line);
		return () => observer.disconnect();
	});

	const delay = (index: number) => `--d: ${Math.round((index * 450) / Math.max(1, count - 1))}ms`;
</script>

<p bind:this={line} class="light {className}" data-light={phase} style="--light-rest: {phase ? rest : on}; --light-on: {on}">
	{#each words as word, i (i)}{#if /^\s+$/.test(word)}{word}{:else}<span class="word" style={delay(words.slice(0, i).filter((w) => !/^\s+$/.test(w)).length)}>{word}</span>{/if}{/each}
</p>

<style>
	.light .word {
		color: var(--light-rest, currentColor);
	}
	.light[data-light='go'] .word {
		color: var(--light-on, currentColor);
		transition: color 250ms cubic-bezier(0.22, 1, 0.36, 1) var(--d);
	}
</style>
