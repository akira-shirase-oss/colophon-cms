<!--
  The docs' navigation: each section and its pages in reading order, the current page marked, a
  page whose subject is being built or planned marked with its step or milestone, the release
  notes and upgrade guides under Updates, and the design documents grouped as in docs/. Used in the
  left column on wide screens and in the menu sheet on phones.
-->
<script lang="ts">
	import ChevronRightIcon from '@lucide/svelte/icons/chevron-right';
	import { DESIGN, SECTIONS } from '#lib/nav.ts';
	import { href } from '#lib/paths.ts';
	import type { DocLink, RepoLinks } from '#lib/types.ts';

	let { current, repo }: { current: string; repo: RepoLinks } = $props();

	const groups = $derived(
		(['Project', 'Modules', 'Formats', 'Brand'] as const)
			.map((group) => ({ group, docs: repo.design.filter((doc) => doc.group === group) }))
			.filter(({ docs }) => docs.length > 0)
	);
	/** Pages listed under one of the Updates pages: release notes under /releases/, guides under /upgrading/. */
	const children = $derived<Record<string, DocLink[]>>({ '/releases/': repo.releases, '/upgrading/': repo.upgrades });

	const link =
		'flex items-center gap-2 rounded-full px-3 py-1.5 text-[0.9375rem] leading-snug text-ink-2 outline-none hover:bg-[var(--chip)] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring aria-[current=page]:bg-[var(--chip)] aria-[current=page]:font-medium aria-[current=page]:text-foreground';
	const heading = 'mb-2 px-3 text-xs leading-4 font-semibold tracking-wide text-[color:var(--eyebrow)] uppercase';
</script>

<nav aria-label="Documentation" class="space-y-7">
	{#each SECTIONS as section (section.title)}
		<div>
			<p class={heading}>{section.title}</p>
			<ul class="space-y-0.5">
				{#each section.pages as page (page.href)}
					<li>
						<a href={href(page.href)} class={link} aria-current={current === page.href ? 'page' : undefined}>
							<span class="min-w-0 flex-1">{page.title}</span>
							{#if page.capability.kind === 'planned'}
								<span class="shrink-0 rounded-full bg-draft-soft px-1.5 text-[0.6875rem] leading-5 font-medium text-draft-ink" title="Planned">{page.capability.at}</span>
							{:else if page.capability.kind === 'building'}
								<span class="shrink-0 rounded-full bg-pending-soft px-1.5 text-[0.6875rem] leading-5 font-medium text-pending-ink" title="Building"
									>{page.capability.at.includes('/') ? 'building' : page.capability.at}</span
								>
							{/if}
						</a>
						{#if (children[page.href] ?? []).length > 0}
							<ul class="mt-0.5 mb-1 ml-5 space-y-0.5 border-l border-[color:var(--line)] pl-2">
								{#each children[page.href] ?? [] as child (child.href)}
									<li><a href={href(child.href)} class={link} aria-current={current === child.href ? 'page' : undefined}>{child.title}</a></li>
								{/each}
							</ul>
						{/if}
					</li>
				{/each}
			</ul>
		</div>
	{/each}

	<div>
		<p class={heading}>{DESIGN.title}</p>
		<ul class="space-y-0.5">
			<li>
				<a href={href(DESIGN.href)} class={link} aria-current={current === DESIGN.href ? 'page' : undefined}>About the design documents</a>
			</li>
			{#each groups as { group, docs } (group)}
				<li>
					<details class="group" open={docs.some((doc) => doc.href === current)}>
						<summary
							class="flex cursor-pointer list-none items-center gap-1.5 rounded-full px-3 py-1.5 text-[0.9375rem] text-ink-2 outline-none select-none hover:bg-[var(--chip)] hover:text-foreground focus-visible:ring-2 focus-visible:ring-ring [&::-webkit-details-marker]:hidden"
						>
							<ChevronRightIcon class="size-3.5 shrink-0 transition-transform group-open:rotate-90" aria-hidden="true" />
							{group}
							<span class="ml-auto text-xs text-muted-foreground tabular-nums">{docs.length}</span>
						</summary>
						<ul class="mt-0.5 ml-5 space-y-0.5 border-l border-[color:var(--line)] pl-2">
							{#each docs as doc (doc.href)}
								<li>
									<a href={href(doc.href)} class={link} aria-current={current === doc.href ? 'page' : undefined}>{doc.title}</a>
								</li>
							{/each}
						</ul>
					</details>
				</li>
			{/each}
		</ul>
	</div>
</nav>
