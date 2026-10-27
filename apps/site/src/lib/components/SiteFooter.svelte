<!--
  The end of every page, after the reference's: one last invitation, then a glass card with the
  mark, a sentence, the project's accounts, the sitemap and the licence's status.
-->
<script lang="ts">
	import { reveal } from '#lib/motion.ts';
	import { BUILD_LOG, DOCS, GITHUB, GITHUB_FILE, QUICKSTART, X_ACCOUNT } from '#lib/site.ts';
	import BrandIcon from './BrandIcon.svelte';
	import Mark from './Mark.svelte';
	import Pill from './Pill.svelte';

	const COLUMNS = [
		{
			title: 'Product',
			links: [
				{ href: '/product/', label: 'Features' },
				{ href: '/compare/', label: 'Compare' },
				{ href: '/roadmap/', label: 'Roadmap' },
				{ href: '/open-source/', label: 'Open source' }
			]
		},
		{
			title: 'Updates',
			links: [
				{ href: '/changelog/', label: 'Changelog' },
				{ href: '/blog/', label: 'Blog' },
				{ href: '/blog/rss.xml', label: 'RSS feed' },
				{ href: BUILD_LOG, label: 'Build log' }
			]
		},
		{
			title: 'Project',
			links: [
				{ href: DOCS, label: 'Docs', external: true },
				{ href: GITHUB, label: 'GitHub' },
				{ href: GITHUB_FILE('docs/DECISIONS.md'), label: 'Decisions' },
				{ href: GITHUB_FILE('AGENTS.md'), label: 'How it is built' }
			]
		}
	];
</script>

<footer class="mt-20 tb:mt-24 dt:mt-40">
	<section aria-labelledby="footer-start" class="shell text-center" {@attach reveal}>
		<h2 id="footer-start" class="t-h2">Try it on your laptop</h2>
		<p class="t-body-lg mx-auto mt-4 max-w-[50rem]">The CMS, the Studio and a site that rebuilds on every release, all running locally.</p>
		<div class="mt-8 flex flex-col justify-center gap-2 tb:flex-row">
			<Pill label="Read the quickstart" href={QUICKSTART} external />
		</div>
	</section>

	<div class="shell mt-24 pb-16 dt:mt-[7.5rem]">
		<div class="footer-card rounded-[32px] p-6 tb:p-10">
			<div class="flex flex-col gap-10 tb:flex-row tb:justify-between">
				<div class="max-w-[17rem]">
					<a href="/" class="inline-flex items-center gap-1.5 rounded-full text-xl leading-6 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring">
						<Mark size={22} />Colophon<span class="sr-only">, home</span>
					</a>
					<p class="t-body mt-4">An open-source CMS that can prove what it published. Pre-release.</p>
					<ul class="mt-6 flex gap-3">
						<li>
							<a href={GITHUB} aria-label="Colophon on GitHub" class="social grid size-10 place-items-center rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
								<BrandIcon name="github" size={18} />
							</a>
						</li>
						<li>
							<a href={X_ACCOUNT} aria-label="The project on X, @akirashiraseoss" class="social grid size-10 place-items-center rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
								<BrandIcon name="x" size={16} />
							</a>
						</li>
					</ul>
				</div>
				<div class="grid grid-cols-2 gap-x-6 gap-y-10 tb:grid-cols-3 tb:gap-8">
					{#each COLUMNS as column (column.title)}
						<nav aria-label={column.title} class="tb:w-[8.375rem]">
							<h2 class="t-label text-[0.9375rem] text-foreground">{column.title}</h2>
							<ul class="mt-4 grid gap-4">
								{#each column.links as link (link.href)}
									<li>
										<a
											href={link.href}
											rel={'external' in link && link.external ? 'external' : undefined}
											class="footer-link rounded text-base leading-5 outline-none focus-visible:ring-2 focus-visible:ring-ring">{link.label}</a
										>
									</li>
								{/each}
							</ul>
						</nav>
					{/each}
				</div>
			</div>
			<div class="t-body mt-12 flex flex-col gap-3 border-t border-[color:var(--nav-edge)] pt-8 text-center tb:mt-20 tb:flex-row tb:justify-between tb:text-left">
				<p>Licence: Apache-2.0 is proposed, not yet confirmed (O-014).</p>
				<p>No analytics, no cookies, nothing loaded from another site.</p>
			</div>
		</div>
	</div>
</footer>

<style>
	.footer-card {
		background: var(--nav-glass);
		box-shadow: inset 0 0 0 1px var(--nav-edge);
		-webkit-backdrop-filter: blur(5px);
		backdrop-filter: blur(5px);
	}
	.social {
		background: var(--primary);
		color: var(--primary-foreground);
		box-shadow: var(--shadow-icon);
		transition: background-color 200ms ease;
	}
	.social:hover {
		background: color-mix(in srgb, var(--primary) 85%, transparent);
	}
	.footer-link {
		color: var(--ink-2);
		transition: color 400ms cubic-bezier(0.44, 0, 0.56, 1);
	}
	.footer-link:hover {
		color: var(--foreground);
	}
</style>
