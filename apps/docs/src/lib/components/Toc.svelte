<!--
  "On this page": the page's h2s and h3s. The heading you are reading is marked as you scroll
  (a reading aid only; the links work without it).
-->
<script lang="ts">
	import type { TocEntry } from '#lib/markdown/structure.ts';

	let { entries, label = 'On this page' }: { entries: TocEntry[]; label?: string } = $props();

	let active = $state('');

	$effect(() => {
		const targets = entries.map((entry) => document.getElementById(entry.id)).filter((node) => node !== null);
		if (targets.length === 0 || !('IntersectionObserver' in window)) return;
		const observer = new IntersectionObserver(
			(seen) => {
				const top = seen.filter((item) => item.isIntersecting).sort((a, b) => a.boundingClientRect.top - b.boundingClientRect.top)[0];
				if (top) active = top.target.id;
			},
			{ rootMargin: '-80px 0px -65% 0px' }
		);
		for (const target of targets) observer.observe(target);
		return () => observer.disconnect();
	});
</script>

{#if entries.length > 0}
	<nav aria-label={label}>
		<p class="mb-2 text-xs font-semibold tracking-tight text-foreground">{label}</p>
		<ul class="space-y-1 border-l border-[color:var(--line)] text-[0.8125rem] leading-snug">
			{#each entries as entry (entry.id)}
				<li>
					<a
						href="#{entry.id}"
						class="-ml-px block border-l py-0.5 [overflow-wrap:anywhere] text-muted-foreground outline-none hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring {entry.depth === 3
							? 'pl-6'
							: 'pl-3'} {active === entry.id ? 'border-foreground font-medium text-foreground' : 'border-transparent'}"
						aria-current={active === entry.id ? 'location' : undefined}>{entry.text}</a
					>
				</li>
			{/each}
		</ul>
	</nav>
{/if}
