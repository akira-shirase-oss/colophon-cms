<!--
  The design sheet (W.2): the theme entry's draft rendered as the design system — type, buttons,
  a card, the scheme rows — through the shared renderer's stylesheet, plus the contrast pairs
  that stylesheet pairs up, checked against AA. It is the draft being edited, so picking a colour
  reskins it in place; warnings are advice, not validation — the save still decides.
-->
<script lang="ts">
	import { pageHtml, SITE_CSS, themeCss, type ThemeContent } from '@colophon/site-renderer';
	import type { Content } from './api.ts';

	let { content }: { content: Content } = $props();

	// A fixed little page that exercises every token the stylesheet consumes — the point is the
	// theme, so the markup stays static and only the variables change.
	const DEMO_BODY = `<nav class="site-nav"><ul class="site-nav-list"><li><a href="#">Home</a></li><li><a href="#">About</a></li><li><a href="#">Blog</a></li></ul></nav>
<section class="site-section site-hero"><div class="site-hero-inner">
<h1>Heading in the heading font</h1>
<p class="site-tagline">A tagline in the muted colour, at the body size.</p>
<a class="site-btn" href="#">A primary button</a>
</div></section>
<section class="site-section site-features"><h2>A features row</h2>
<ul class="site-feature-grid"><li class="site-card"><h3>A card</h3>
<p>Card corners use the large radius and the theme's shadow.</p></li><li class="site-card"><h3>Another</h3>
<p>Body text is set in the body font.</p></li></ul></section>
<section class="site-section" data-scheme="primary"><h2>A primary section</h2>
<p>White text on the primary colour, like a call to action.</p>
<a class="site-btn" href="#">Its button inverts</a></section>
<section class="site-section" data-scheme="secondary"><h2>A secondary section</h2>
<p>White text on the secondary colour.</p></section>
<section class="site-section" data-scheme="invert"><h2>An inverted section</h2>
<p>The surface colour on the text colour.</p></section>`;

	let markup = $state('');
	let problem = $state('');
	const theme = $derived(content as ThemeContent);
	$effect(() => {
		const draft = theme;
		const timer = setTimeout(() => {
			try {
				markup = pageHtml({
					title: 'The site’s design',
					head: `<style>${themeCss(draft)}${SITE_CSS}</style>\n`,
					body: DEMO_BODY
				});
				problem = '';
			} catch (error) {
				problem = error instanceof Error ? error.message : String(error);
			}
		}, 200);
		return () => clearTimeout(timer);
	});

	// WCAG AA contrast: the relative-luminance formula over the #rrggbb the theme already
	// guarantees. The pairs are the ones SITE_CSS actually pairs — a warning names its pair.
	function luminance(hex: string): number {
		const channel = (index: number) => {
			const value = parseInt(hex.slice(index, index + 2), 16) / 255;
			return value <= 0.04045 ? value / 12.92 : Math.pow((value + 0.055) / 1.055, 2.4);
		};
		return 0.2126 * channel(1) + 0.7152 * channel(3) + 0.0722 * channel(5);
	}
	function ratio(a: string, b: string): number {
		const [high, low] = [luminance(a), luminance(b)].sort((x, y) => y - x);
		return ((high ?? 0) + 0.05) / ((low ?? 0) + 0.05);
	}
	interface Pair {
		what: string;
		foreground: string;
		background: string;
	}
	const contrast = $derived.by(() => {
		const light = theme.light;
		if (light === undefined || light === null) return [];
		const hex = (value: unknown): string | null => (typeof value === 'string' && /^#[0-9a-f]{6}$/i.test(value) ? value : null);
		const surface = hex(light.surface);
		const text = hex(light.text);
		const muted = hex(light.muted);
		const primary = hex(light.primary);
		const secondary = hex(light.secondary);
		const pairs: Pair[] = [];
		if (text !== null && surface !== null) {
			pairs.push({ what: 'body text', foreground: text, background: surface });
			pairs.push({ what: 'inverted sections', foreground: surface, background: text });
		}
		if (muted !== null && surface !== null) pairs.push({ what: 'muted text — taglines, captions', foreground: muted, background: surface });
		if (primary !== null) {
			pairs.push({ what: 'buttons and primary sections', foreground: '#ffffff', background: primary });
			if (surface !== null) pairs.push({ what: 'links and accents', foreground: primary, background: surface });
		}
		if (secondary !== null) pairs.push({ what: 'secondary sections', foreground: '#ffffff', background: secondary });
		return pairs.map((pair) => ({ ...pair, score: ratio(pair.foreground, pair.background) }));
	});
	const weak = $derived(contrast.filter((pair) => pair.score < 4.5));
</script>

<p class="mb-3 text-sm text-muted-foreground">
	The site’s design as you're editing it — every section kind reads these variables. Readers get it once the theme is published.
</p>
{#if problem !== ''}
	<p role="alert" class="rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive" data-testid="theme-problem">
		This theme can't be rendered: {problem}
	</p>
{:else}
	<iframe title="The site's design as you're editing it" srcdoc={markup} class="h-[30rem] w-full rounded-xl border border-border bg-white" data-testid="theme-canvas"></iframe>
{/if}
{#if weak.length > 0}
	<div class="mt-3 rounded-lg border border-attention/40 bg-attention-soft/60 px-4 py-3 text-sm text-attention-ink" data-testid="contrast-warnings">
		<p class="font-semibold">Hard to read: {weak.length === 1 ? 'this pair is' : 'these pairs are'} under the 4.5:1 contrast AA asks for</p>
		<ul class="mt-1.5 list-disc space-y-0.5 pl-5">
			{#each weak as pair (pair.what)}<li>{pair.what} — {pair.score.toFixed(2)}:1 (<span class="font-mono">{pair.foreground}</span> on <span class="font-mono">{pair.background}</span>)</li>{/each}
		</ul>
	</div>
{/if}
