<script lang="ts">
	import { page } from '$app/state';
	import { resolve } from '$app/paths';
	import mark from '#lib/colophon-mark.svg';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();

	const links = [
		{ href: resolve('/'), label: 'Health' },
		{ href: resolve('/contract'), label: 'API contract' },
		{ href: resolve('/rich-text'), label: 'Rich text' },
		{ href: resolve('/session'), label: 'Session' },
		{ href: resolve('/entries'), label: 'Entries' },
		{ href: resolve('/content-types'), label: 'Content types' },
		{ href: resolve('/device'), label: 'Device sign-in' },
		{ href: resolve('/members'), label: 'Members' },
		{ href: resolve('/changesets'), label: 'Changesets' },
		{ href: resolve('/releases'), label: 'Releases' },
		{ href: resolve('/load'), label: 'Load tests' }
	];
</script>

<svelte:head>
	<link rel="icon" href={mark} />
	<title>Colophon testbed</title>
</svelte:head>

<header>
	<img src={mark} alt="" width="22" height="24" />
	<strong>Colophon testbed</strong>
	<nav aria-label="Testbed pages">
		{#each links as link (link.href)}
			<a href={link.href} aria-current={page.url.pathname === link.href ? 'page' : undefined}>{link.label}</a>
		{/each}
	</nav>
</header>

<main>
	{@render children()}
</main>

<style>
	:global(:root) {
		--bg: #fbfbfa;
		--fg: #1b1c1e;
		--muted: #5f6368;
		--line: #e3e3e0;
		--card: #ffffff;
		--ok: #1e7f4f;
		--ok-bg: #e5f4ec;
		--warn: #8a5a00;
		--warn-bg: #fdf1d6;
		--bad: #b3261e;
		--bad-bg: #fbe7e5;
		color-scheme: light dark;
		font-family:
			system-ui,
			-apple-system,
			'Segoe UI',
			sans-serif;
		background: var(--bg);
		color: var(--fg);
	}
	@media (prefers-color-scheme: dark) {
		:global(:root) {
			--bg: #121314;
			--fg: #e8e8e6;
			--muted: #a0a3a8;
			--line: #2c2e31;
			--card: #1a1b1d;
			--ok: #7fd3a6;
			--ok-bg: #12301f;
			--warn: #f2c35b;
			--warn-bg: #3a2c0a;
			--bad: #f2a29b;
			--bad-bg: #3d1512;
		}
	}
	:global(body) {
		margin: 0;
	}
	:global(code),
	:global(pre) {
		font-family: ui-monospace, 'SF Mono', Menlo, monospace;
		font-size: 0.85em;
	}
	header {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		padding: 0.8rem 1rem;
		border-bottom: 1px solid var(--line);
		flex-wrap: wrap;
	}
	nav {
		display: flex;
		gap: 0.25rem;
		margin-left: auto;
		flex-wrap: wrap;
	}
	nav a {
		color: var(--muted);
		text-decoration: none;
		padding: 0.35rem 0.6rem;
		border-radius: 6px;
	}
	nav a[aria-current='page'] {
		color: var(--fg);
		background: var(--line);
	}
	img {
		filter: none;
	}
	@media (prefers-color-scheme: dark) {
		img {
			filter: invert(1);
		}
	}
	main {
		max-width: 64rem;
		margin: 0 auto;
		padding: 1.25rem 1rem 3rem;
	}
</style>
