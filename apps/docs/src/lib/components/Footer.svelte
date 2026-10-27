<!--
  The end of every page, after the website's footer (apps/site SiteFooter): a glass card with the
  mark, a sentence, the project's accounts, the docs' and the website's main pages, the licence's
  status, and the commit the labels describe.
-->
<script lang="ts">
	import { REPOSITORY } from '#lib/nav.ts';
	import { href, site } from '#lib/paths.ts';
	import BrandIcon from './BrandIcon.svelte';
	import Mark from './Mark.svelte';

	const BUILD_LOG = 'https://404human.dev/colophon/';
	const X_ACCOUNT = 'https://x.com/akirashiraseoss';

	const COLUMNS: { title: string; links: { href: string; label: string; external?: boolean; reload?: boolean }[] }[] = [
		{
			title: 'Docs',
			links: [
				{ href: href('/get-started/quickstart/'), label: 'Run it locally' },
				{ href: href('/editors/'), label: 'Editor guide' },
				{ href: href('/concepts/'), label: 'Concepts' },
				{ href: href('/reference/'), label: 'Reference' },
				{ href: href('/operations/'), label: 'Operations' }
			]
		},
		{
			title: 'Updates',
			links: [
				{ href: href('/changelog/'), label: 'Changelog' },
				{ href: href('/changelog/feed.xml'), label: 'Atom feed', reload: true },
				{ href: href('/releases/'), label: 'Release notes' },
				{ href: href('/upgrading/'), label: 'Upgrade guides' },
				{ href: BUILD_LOG, label: 'Build log', external: true }
			]
		},
		{
			title: 'Website',
			links: [
				{ href: site('/'), label: 'Home', external: true },
				{ href: site('/product/'), label: 'Product', external: true },
				{ href: site('/compare/'), label: 'Compare', external: true },
				{ href: site('/roadmap/'), label: 'Roadmap', external: true },
				{ href: site('/blog/'), label: 'Blog', external: true }
			]
		}
	];
</script>

<footer class="mt-24 px-2 pb-10 sm:px-4" data-pagefind-ignore>
	<div class="footer-card mx-auto max-w-[90rem] rounded-[32px] p-6 tb:p-10">
		<div class="flex flex-col gap-10 tb:flex-row tb:justify-between">
			<div class="max-w-[19rem]">
				<a href={site('/')} rel="external" class="inline-flex items-center gap-1.5 rounded-full text-xl leading-6 font-semibold outline-none focus-visible:ring-2 focus-visible:ring-ring">
					<Mark size={22} />Colophon<span class="sr-only">, the website</span>
				</a>
				<p class="t-body mt-4">An open-source CMS that can prove what it published. Pre‑release: v0.1 is not out.</p>
				<ul class="mt-6 flex gap-3">
					<li>
						<a href={REPOSITORY} rel="external" aria-label="Colophon on GitHub" class="social grid size-10 place-items-center rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
							<BrandIcon name="github" size={18} />
						</a>
					</li>
					<li>
						<a href={X_ACCOUNT} rel="external" aria-label="The project on X, @akirashiraseoss" class="social grid size-10 place-items-center rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring">
							<BrandIcon name="x" size={16} />
						</a>
					</li>
				</ul>
			</div>
			<div class="grid grid-cols-2 gap-x-6 gap-y-10 tb:grid-cols-3 tb:gap-10">
				{#each COLUMNS as column (column.title)}
					<nav aria-label="{column.title}, in the footer">
						<h2 class="t-label text-[0.9375rem] text-foreground">{column.title}</h2>
						<ul class="mt-4 grid gap-3.5">
							{#each column.links as link (link.href)}
								<li>
									<a
										href={link.href}
										rel={link.external ? 'external' : undefined}
										data-sveltekit-reload={link.reload ? '' : undefined}
										class="footer-link rounded text-[0.9375rem] leading-5 outline-none focus-visible:ring-2 focus-visible:ring-ring">{link.label}</a
									>
								</li>
							{/each}
						</ul>
					</nav>
				{/each}
			</div>
		</div>
		<div class="t-small mt-12 flex flex-col gap-3 border-t border-[color:var(--nav-edge)] pt-8 text-center tb:mt-16 tb:flex-row tb:justify-between tb:text-left">
			<p>Licence: Apache-2.0 is proposed, not yet confirmed (O-014).</p>
			<p>
				Statuses describe the code at commit
				<a class="font-mono text-foreground underline decoration-[color:var(--line)] underline-offset-4 hover:decoration-foreground" href="{REPOSITORY}/commit/{__COMMIT__}" rel="external">{__COMMIT__}</a>.
			</p>
		</div>
	</div>
</footer>

<style>
	.footer-card {
		background: var(--sand);
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
