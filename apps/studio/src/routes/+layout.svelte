<script lang="ts">
	import mark from '#lib/colophon-mark.svg';
	import type { LayoutProps } from './$types';

	let { children }: LayoutProps = $props();
</script>

<svelte:head>
	<link rel="icon" href={mark} />
	<title>Colophon Studio</title>
</svelte:head>

<header>
	<img src={mark} alt="" width="22" height="24" />
	<strong>Colophon Studio</strong>
	<span class="tag">editor spike · step 0.4</span>
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
		--accent: #2f5bd3;
		--ok: #1e7f4f;
		--ok-bg: #e5f4ec;
		--warn: #8a5a00;
		--warn-bg: #fdf1d6;
		--bad: #b3261e;
		--bad-bg: #fbe7e5;
		color-scheme: light dark;
		font-family: system-ui, -apple-system, 'Segoe UI', sans-serif;
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
			--accent: #8fb0ff;
			--ok: #7fd3a6;
			--ok-bg: #12301f;
			--warn: #f2c35b;
			--warn-bg: #3a2c0a;
			--bad: #f2a29b;
			--bad-bg: #3d1512;
		}
		img {
			filter: invert(1);
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
	}
	.tag {
		margin-left: auto;
		color: var(--muted);
		font-size: 0.85rem;
	}
	main {
		max-width: 76rem;
		margin: 0 auto;
		padding: 1rem 1rem 3rem;
	}
</style>
