<!--
  The site's navigation: each section and its pages in reading order, the current page marked,
  pages that describe planned work marked too, and the design documents grouped as in docs/.
  Used in the left column on wide screens and in the menu sheet on phones.
-->
<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { DESIGN, SECTIONS } from '#lib/nav.ts';
	import type { DesignDocLink } from '#lib/types.ts';

	let { current, design }: { current: string; design: DesignDocLink[] } = $props();

	const groups = $derived(
		(['Project', 'Modules', 'Formats', 'Brand'] as const)
			.map((group) => ({ group, docs: design.filter((doc) => doc.group === group) }))
			.filter(({ docs }) => docs.length > 0)
	);
	const inDesign = $derived(current.startsWith(DESIGN.href));

	const link =
		'flex items-center gap-2 rounded-lg px-2.5 py-1.5 text-[0.875rem] leading-snug text-muted-foreground outline-none hover:bg-accent hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 aria-[current=page]:bg-card aria-[current=page]:font-medium aria-[current=page]:text-foreground aria-[current=page]:shadow-hairline';
</script>

<nav aria-label="Documentation" class="space-y-6 text-sm">
	{#each SECTIONS as section (section.href)}
		<div>
			<p class="mb-1.5 px-2.5 text-xs font-semibold tracking-tight text-foreground">{section.title}</p>
			<ul class="space-y-0.5">
				{#each section.pages as page (page.href)}
					<li>
						<a href={page.href} class={link} aria-current={current === page.href ? 'page' : undefined}>
							<span class="min-w-0 flex-1">{page.title}</span>
							{#if page.capability.kind === 'planned'}
								<span class="shrink-0 rounded-full bg-draft-soft px-1.5 text-[0.6875rem] text-draft-ink">{page.capability.milestone}</span>
							{:else if page.capability.kind === 'building'}
								<span class="shrink-0 rounded-full bg-pending-soft px-1.5 text-[0.6875rem] text-pending-ink">building</span>
							{/if}
						</a>
					</li>
				{/each}
			</ul>
		</div>
	{/each}

	<div>
		<p class="mb-1.5 px-2.5 text-xs font-semibold tracking-tight text-foreground">{DESIGN.title}</p>
		<ul class="space-y-0.5">
			<li>
				<a href={DESIGN.href} class={link} aria-current={current === DESIGN.href ? 'page' : undefined}>About the design documents</a>
			</li>
			{#each groups as { group, docs } (group)}
				<li>
					<details class="group" open={inDesign && docs.some((doc) => doc.href === current)}>
						<summary
							class="flex cursor-pointer list-none items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[0.875rem] text-muted-foreground outline-none select-none hover:bg-accent hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 [&::-webkit-details-marker]:hidden"
						>
							<ChevronRightIcon class="size-3.5 shrink-0 transition-transform group-open:rotate-90" aria-hidden="true" />
							{group}
							<span class="ml-auto text-xs tabular-nums">{docs.length}</span>
						</summary>
						<ul class="mt-0.5 ml-4 space-y-0.5 border-l border-border pl-2">
							{#each docs as doc (doc.href)}
								<li>
									<a href={doc.href} class={link} aria-current={current === doc.href ? 'page' : undefined}>{doc.title}</a>
								</li>
							{/each}
						</ul>
					</details>
				</li>
			{/each}
		</ul>
	</div>
</nav>
