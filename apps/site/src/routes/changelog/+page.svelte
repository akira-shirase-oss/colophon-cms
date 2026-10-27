<script lang="ts">
	import GitBranchIcon from '@lucide/svelte/icons/git-branch';
	import GitMergeIcon from '@lucide/svelte/icons/git-merge';
	import { Badge } from '@colophon/ui/ui/badge';
	import PageIntro from '#lib/components/PageIntro.svelte';
	import Seo from '#lib/components/Seo.svelte';
	import { longDate } from '#lib/blog.ts';
	import { BUILD_LOG, GITHUB_COMMIT, GITHUB_FILE } from '#lib/site.ts';
	import type { PageProps } from './$types';

	let { data }: PageProps = $props();
	const sections = $derived(data.changelog.sections);
	const count = $derived(sections.reduce((total, section) => total + section.entries.length, 0));
	// A build-log link names the post's original; its copy on this site is one click closer.
	const local = (url: string) => (url.startsWith(BUILD_LOG) ? `/blog/${url.slice(BUILD_LOG.length)}` : url);
</script>

<Seo
	title="Changelog · Colophon"
	description="Every step of Colophon CMS merged into main, newest first, with its commits, decisions and build-log post; and what is in progress."
/>

<PageIntro badge="Changelog" title="What changed, step by step">
	<p>
		Every step merged into <code class="font-mono text-[0.9em]">main</code>, newest first, from the repository’s own history: {count} entries, read at build time from
		<a class="text-link" href={GITHUB_FILE('CHANGELOG.md')}>CHANGELOG.md</a>. There are no version numbers yet: v0.1 is not out.
	</p>
</PageIntro>

<div class="shell mt-12 grid gap-12 lg:grid-cols-[12rem_minmax(0,1fr)]">
	<nav aria-label="Changelog sections" class="hidden lg:block">
		<ul class="sticky top-24 grid gap-1 text-sm">
			{#each sections as section (section.id)}
				<li>
					<a href="#{section.id}" class="block rounded-xl px-3 py-2 text-muted-foreground hover:bg-accent hover:text-foreground">
						{section.unreleased ? 'In progress' : section.title.split(' · ')[0]}
						<span class="figure text-xs">· {section.entries.length}</span>
					</a>
				</li>
			{/each}
		</ul>
	</nav>

	<div>
		{#each sections as section (section.id)}
			<section aria-labelledby={section.id} class="mb-16">
				<div class="mb-6 flex flex-wrap items-center gap-3">
					<h2 id={section.id} class="display-2">{section.unreleased ? 'Unreleased, in progress' : section.title}</h2>
					{#if section.unreleased}<Badge variant="pending">Not merged</Badge>{/if}
				</div>
				<ol class="relative grid gap-6 border-l border-border pl-6 sm:pl-8">
					{#each section.entries as entry (entry.id)}
						<li id={entry.id} data-testid="changelog-entry" data-step={entry.step} class="relative">
							<span
								class="absolute top-6 -left-[1.6875rem] grid size-3 place-items-center rounded-full ring-4 ring-background sm:-left-[2.1875rem] {entry.status === 'merged' ? 'bg-live' : 'bg-pending'}"
								aria-hidden="true"
							></span>
							<article class="rounded-3xl border border-border/70 bg-card p-6 shadow-soft sm:p-7">
								<div class="flex flex-wrap items-center gap-2 text-sm text-muted-foreground">
									<span class="figure rounded-full bg-primary px-2.5 py-0.5 text-xs font-medium text-primary-foreground">{entry.step}</span>
									{#if entry.status === 'merged'}
										<Badge variant="live"><GitMergeIcon aria-hidden="true" />Merged</Badge>
										{#if entry.date}<time datetime={entry.date}>{longDate(`${entry.date}T00:00:00Z`)}</time>{/if}
									{:else}
										<Badge variant="pending"><GitBranchIcon aria-hidden="true" />{entry.status[0]?.toUpperCase()}{entry.status.slice(1)}</Badge>
										{#if entry.branch}<code class="font-mono text-xs">{entry.branch}</code>{/if}
									{/if}
								</div>
								<h3 class="title mt-3" data-testid="changelog-title">{entry.title}</h3>
								<div class="prose-site changelog-body mt-3 text-[0.9375rem]">
									<!-- CHANGELOG.md, rendered at build time with raw HTML escaped (src/lib/server/markdown.ts). -->
									{@html entry.html}
								</div>
								<dl class="mt-5 flex flex-wrap gap-x-6 gap-y-2 border-t border-border pt-4 text-xs text-muted-foreground">
									{#if entry.commits.length > 0}
										<div class="flex gap-1.5">
											<dt>Merge {entry.commits.length === 1 ? 'commit' : 'commits'}</dt>
											<dd class="flex gap-1.5">
												{#each entry.commits as commit (commit)}
													<a class="font-mono underline underline-offset-2 hover:text-foreground" href={GITHUB_COMMIT(commit)}>{commit}</a>
												{/each}
											</dd>
										</div>
									{/if}
									{#if entry.decisions}
										<div class="flex gap-1.5">
											<dt>Decisions</dt>
											<dd><a class="underline underline-offset-2 hover:text-foreground" href={GITHUB_FILE('docs/DECISIONS.md')}>{entry.decisions}</a></dd>
										</div>
									{/if}
									{#if entry.buildLog}
										<div>
											<dt class="sr-only">Build log</dt>
											<dd>
												<a class="underline underline-offset-2 hover:text-foreground" href={local(entry.buildLog)}>Build-log post</a>
											</dd>
										</div>
									{/if}
								</dl>
							</article>
						</li>
					{/each}
				</ol>
			</section>
		{/each}
	</div>
</div>

<style>
	.changelog-body :global(p),
	.changelog-body :global(li) {
		color: var(--muted-foreground);
	}
	.changelog-body {
		max-width: none;
		font-size: 0.9375rem;
		line-height: 1.65;
	}
</style>
