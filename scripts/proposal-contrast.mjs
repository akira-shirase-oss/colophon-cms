#!/usr/bin/env node
// Studio v2 proposal: every new text/surface pair and every new mark, in both themes, measured the way
// scripts/design-contrast.mjs measures D-210's (it is imported from there), from design/tokens.css with the
// proposal's own tokens (apps/studio/src/lib/proposal/studio-v2.css) over it. Exits 1 on a pair below its bar.
// When the maintainer picks a direction, its tokens move into design/tokens.css and these rows into the table.
//
//   node scripts/proposal-contrast.mjs
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { block, linear, ratio, hex } from './design-contrast.mjs';

const read = (path) => readFileSync(fileURLToPath(new URL(`../${path}`, import.meta.url)), 'utf8');
const tokens = read('design/tokens.css');
const proposal = read('apps/studio/src/lib/proposal/studio-v2.css').replace(/\/\*[\s\S]*?\*\//g, '');

// Each theme's tokens with every `var(--x)` resolved in that theme.
const resolve = (t) => {
	const get = (name, seen = 0) => {
		const v = t[name];
		const m = v?.match(/^var\(--([a-z0-9-]+)\)$/);
		if (seen > 10) throw new Error(`--${name} refers to itself`);
		return m ? get(m[1], seen + 1) : v;
	};
	return Object.fromEntries(Object.keys(t).map((name) => [name, get(name)]));
};
const light = resolve({ ...block(tokens, ':root'), ...block(proposal, ':root') });
const dark = resolve({ ...block(tokens, ':root'), ...block(proposal, ':root'), ...block(tokens, '.dark'), ...block(proposal, '.dark') });

/** A ground: a token, or `name/NN over base` blended in sRGB as the browser does. */
function ground(t, name) {
	const m = name.match(/^([a-z0-9-]+)\/(\d+) over ([a-z0-9-]+)$/);
	if (!m) return t[name];
	const encode = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
	const [top, base, a] = [linear(t[m[1]]).map(encode), linear(t[m[3]]).map(encode), Number(m[2]) / 100];
	return '#' + top.map((c, i) => Math.round((a * c + (1 - a) * base[i]) * 255).toString(16).padStart(2, '0')).join('');
}

const TEXT = [
	// The inverted Get-started panel: its title and steps, its muted lines, a label on the 9% hover ground,
	// white on the one primary button inside it.
	['hero-fg', 'hero-bg'], ['hero-muted', 'hero-bg'], ['hero-fg', 'hero-fg/9 over hero-bg'],
	['primary-foreground', 'primary'],
	// Cards and rows on the section ground, and on its hover.
	['foreground', 'surface-section'], ['muted-foreground', 'surface-section'],
	['foreground', 'surface-section-hover'], ['muted-foreground', 'surface-section-hover'],
	['draft-ink', 'draft-soft'],
	// B: the sidebar on the inverted ground, its selected item and live card on the 12% step.
	['hero-fg', 'hero-fg/12 over hero-bg'], ['hero-muted', 'hero-fg/12 over hero-bg']
];
const MARKS = [
	// The panel's outline buttons (a 1px edge of its ink), the focus ring, the blue button and the step tiles' edge.
	['hero-fg', 'hero-bg'], ['ring', 'hero-bg'], ['primary', 'hero-bg'],
	// The pixel icons' outline on the page, the section ground and (as the panel's ink) the panel.
	['px-outline', 'background'], ['px-outline', 'surface-section'],
	// B: the selected item's icon on the sidebar's step (--brand on black, --primary on white).
	['v2-nav-active', 'hero-fg/12 over hero-bg']
];

let failed = 0;
for (const [theme, t] of [['light', light], ['dark', dark]]) {
	for (const [pairs, bar] of [[TEXT, 4.5], [MARKS, 3]]) {
		for (const [fg, bg] of pairs) {
			const g = ground(t, bg);
			if (t[fg] === undefined || g === undefined) throw new Error(`${theme}: --${fg} or --${bg} is missing`);
			const r = ratio(t[fg], g.startsWith('#') ? g : g);
			const pass = r >= bar;
			if (!pass) failed++;
			console.log(`${pass ? 'pass' : 'FAIL'}  ${theme.padEnd(5)} --${fg} on --${bg}: ${r.toFixed(2)}:1 (bar ${bar}) ${hex(t[fg])} on ${g.startsWith('#') ? g : hex(g)}`);
		}
	}
}
if (failed) {
	console.error(`${failed} pair(s) below their bar`);
	process.exit(1);
}
