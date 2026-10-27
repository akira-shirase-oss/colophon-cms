<!-- The sitemap, the status in one line, and where the facts come from. -->
<script lang="ts">
	import { BUILD_LOG, DOCS, GITHUB, GITHUB_FILE } from '#lib/site.ts';
	import Mark from './Mark.svelte';

	const COLUMNS = [
		{
			title: 'Product',
			links: [
				{ href: '/product/', label: 'Features' },
				{ href: '/compare/', label: 'Compare with Strapi and Sanity' },
				{ href: '/roadmap/', label: 'Roadmap' }
			]
		},
		{
			title: 'Updates',
			links: [
				{ href: '/changelog/', label: 'Changelog' },
				{ href: '/blog/', label: 'Blog' },
				{ href: '/blog/rss.xml', label: 'RSS feed' },
				{ href: BUILD_LOG, label: 'Build log at 404human.dev' }
			]
		},
		{
			title: 'Project',
			links: [
				{ href: '/open-source/', label: 'Open source and self-hosting' },
				{ href: DOCS, label: 'Docs', external: true },
				{ href: GITHUB, label: 'GitHub repository' },
				{ href: GITHUB_FILE('docs/DECISIONS.md'), label: 'Decisions' }
			]
		}
	];
</script>

<footer class="mt-24 border-t border-border bg-card/60">
	<div class="shell grid grid-cols-2 gap-x-6 gap-y-10 py-14 md:grid-cols-[minmax(0,1.2fr)_repeat(3,minmax(0,1fr))] md:gap-12">
		<div class="col-span-2 space-y-4 md:col-span-1">
			<a href="/" class="inline-flex items-center gap-2.5 rounded-lg font-semibold tracking-tight outline-none focus-visible:ring-4 focus-visible:ring-ring/25">
				<Mark size={20} />Colophon<span class="sr-only">, home</span>
			</a>
			<p class="max-w-xs text-sm leading-relaxed text-muted-foreground">
				An open-source CMS that can prove what it published. Pre-release: every feature on this site is labelled done, building or planned.
			</p>
			<p class="text-xs text-muted-foreground">Licence: Apache-2.0 is proposed, not yet confirmed (O-014).</p>
		</div>
		{#each COLUMNS as column (column.title)}
			<nav aria-label={column.title}>
				<h2 class="text-sm font-semibold">{column.title}</h2>
				<ul class="mt-4 grid gap-2.5">
					{#each column.links as link (link.href)}
						<li>
							<a
								href={link.href}
								rel={'external' in link && link.external ? 'external' : undefined}
								class="rounded text-sm text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-4 focus-visible:ring-ring/25"
							>
								{link.label}
							</a>
						</li>
					{/each}
				</ul>
			</nav>
		{/each}
	</div>
	<div class="shell flex flex-col gap-2 border-t border-border py-6 text-xs text-muted-foreground sm:flex-row sm:justify-between">
		<p>Built in the open by one maintainer pairing with Claude Code. No analytics, no cookies, nothing loaded from another site.</p>
		<p>This site is prerendered from the repository: the changelog, the blog and every number come from its files.</p>
	</div>
</footer>
