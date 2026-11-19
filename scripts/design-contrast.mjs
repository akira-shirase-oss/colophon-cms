#!/usr/bin/env node
// The contrast table (D-210): every text/surface pair and every non-text boundary in both themes, measured from
// design/tokens.css (WCAG 2.x relative luminance, oklch converted to sRGB the CSS Color 4 way, out-of-gamut
// channels clipped as a browser shows them). Text needs 4.5:1 (WCAG 1.4.3); field boundaries, focus rings, icons
// and chart marks need 3:1 (1.4.11).
//
//   node scripts/design-contrast.mjs                   # prints the table; exits 1 if any pair fails its bar
//   node scripts/design-contrast.mjs --write <file>    # also writes the table as Markdown (docs/design/contrast.md)
//   node scripts/design-contrast.mjs --check <file>    # exits 1 if <file> isn't what --write would write
//   node scripts/design-contrast.mjs --check-copy <f>  # exits 1 if <f> isn't a byte copy of design/tokens.css
//                                                      # (packages/ui's copy here; the landing's and the docs' in
//                                                      # their own CI, against this repository's file)
import { readFileSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';

const source = fileURLToPath(new URL('../design/tokens.css', import.meta.url));

/** The `--name: value` declarations of one block (`:root` or `.dark`). */
export function block(css, selector) {
	const start = css.indexOf(`${selector} {`);
	if (start < 0) throw new Error(`no ${selector} block in design/tokens.css`);
	const body = css.slice(start, css.indexOf('}', start));
	const tokens = {};
	for (const m of body.matchAll(/--([a-z0-9-]+)\s*:\s*([^;]+);/g)) tokens[m[1]] = m[2].trim();
	return tokens;
}

/** A token's colour as linear sRGB, clipped to the gamut. */
export function linear(value) {
	let m = value.match(/^oklch\(\s*([\d.]+)\s+([\d.]+)\s+([\d.]+)\s*\)$/);
	if (m) {
		const [L, C, h] = m.slice(1).map(Number);
		const a = C * Math.cos((h * Math.PI) / 180);
		const b = C * Math.sin((h * Math.PI) / 180);
		const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
		const mm = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
		const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
		return [
			4.0767416621 * l - 3.3077115913 * mm + 0.2309699292 * s,
			-1.2684380046 * l + 2.6097574011 * mm - 0.3413193965 * s,
			-0.0041960863 * l - 0.7034186147 * mm + 1.707614701 * s
		].map((x) => Math.min(1, Math.max(0, x)));
	}
	m = value.match(/^#([0-9a-f]{6})$/i);
	if (m) {
		return [0, 2, 4].map((i) => {
			const c = parseInt(m[1].slice(i, i + 2), 16) / 255;
			return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
		});
	}
	throw new Error(`can't measure ${value}`);
}

export const luminance = (value) => {
	const [r, g, b] = linear(value);
	return 0.2126 * r + 0.7152 * g + 0.0722 * b;
};
export const ratio = (x, y) => {
	const [hi, lo] = [luminance(x), luminance(y)].sort((p, q) => q - p);
	return (hi + 0.05) / (lo + 0.05);
};
export const hex = (value) =>
	'#' +
	linear(value)
		.map((c) => Math.round((c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055) * 255))
		.map((n) => n.toString(16).padStart(2, '0').toUpperCase())
		.join('');

// What sits on what. Text pairs first (4.5), then boundaries and marks (3).
const STATUSES = ['live', 'changed', 'pending', 'attention', 'draft'];
const TEXT = [
	['foreground', 'background'], ['card-foreground', 'card'], ['popover-foreground', 'popover'],
	['primary-foreground', 'primary'], ['secondary-foreground', 'secondary'], ['muted-foreground', 'muted'],
	['muted-foreground', 'background'], ['muted-foreground', 'card'], ['muted-foreground', 'secondary'],
	['muted-foreground', 'brand-soft'], ['accent-foreground', 'accent'],
	['destructive-foreground', 'destructive'], ['destructive', 'background'], ['destructive', 'card'],
	// Error text on the grounds it sits on: plain surfaces and its own tints behind an error (bg-destructive/5, /10).
	['destructive', 'popover'], ['destructive', 'muted'], ['destructive', 'secondary'],
	...['card', 'background', 'popover'].flatMap((g) => [['destructive', `destructive/5 over ${g}`], ['destructive', `destructive/10 over ${g}`]]),
	// Text on the other translucent grounds the Studio and the kit use (grep "bg-[a-z-]+/[0-9]+"): a tint is
	// measured over the surfaces it sits on, never as the token alone.
	...['card', 'background'].flatMap((g) => [
		...['muted/40', 'muted/50', 'muted/60', 'secondary/40'].flatMap((t) => [['foreground', `${t} over ${g}`], ['muted-foreground', `${t} over ${g}`]]),
		...['primary/5', 'primary/10'].flatMap((t) => [['foreground', `${t} over ${g}`], ['muted-foreground', `${t} over ${g}`]]),
		['attention-ink', `attention-soft/60 over ${g}`], ['muted-foreground', `attention-soft/60 over ${g}`],
		['live-ink', `live-soft/50 over ${g}`], ['muted-foreground', `live-soft/50 over ${g}`],
		['pending-ink', `pending-soft/70 over ${g}`], ['muted-foreground', `pending-soft/70 over ${g}`],
		['brand-ink', `brand-soft/40 over ${g}`], ['foreground', `brand-soft/40 over ${g}`],
		['destructive-foreground', `destructive/90 over ${g}`]
	]),
	['accent-foreground', 'accent/50 over popover'],
	 ['link', 'background'], ['link', 'card'], ['link', 'muted'],
	['foreground', 'muted'], ['foreground', 'secondary'], ['foreground', 'accent'],
	['sidebar-foreground', 'sidebar'], ['sidebar-primary-foreground', 'sidebar-primary'],
	['sidebar-accent-foreground', 'sidebar-accent'],
	['primary-foreground', 'primary-hover'], ['brand-ink', 'background'], ['brand-ink', 'card'],
	['brand-ink', 'brand-soft'],
	...STATUSES.flatMap((s) => [[`${s}-ink`, `${s}-soft`], [`${s}-ink`, 'background'], [`${s}-ink`, 'card'], ['muted-foreground', `${s}-soft`]])
];
const MARKS = [
	['input', 'background'], ['input', 'card'], ['ring', 'background'], ['ring', 'card'],
	['brand', 'background'], ['brand', 'card'], ['sidebar-ring', 'sidebar'],
	...[1, 2, 3, 4, 5].map((n) => [`chart-${n}`, 'background']),
	...[1, 2, 3, 4, 5].map((n) => [`chart-${n}`, 'card']),
	['control', 'background'], ['control', 'card'],
	...STATUSES.flatMap((s) => [[s, 'background'], [s, 'card']])
];

/** A ground: a token, or `name/NN over base`, the token at NN% over another, blended as the browser does (in sRGB). */
function tint(t, ground) {
	const m = ground.match(/^([a-z0-9-]+)\/(\d+) over ([a-z0-9-]+)$/);
	if (!m) return t[ground];
	if (!(m[1] in t) || !(m[3] in t)) return undefined;
	const encode = (c) => (c <= 0.0031308 ? 12.92 * c : 1.055 * c ** (1 / 2.4) - 0.055);
	const [top, base, a] = [linear(t[m[1]]).map(encode), linear(t[m[3]]).map(encode), Number(m[2]) / 100];
	return '#' + top.map((c, i) => Math.round((a * c + (1 - a) * base[i]) * 255).toString(16).padStart(2, '0')).join('').toUpperCase();
}

export function table(css) {
	const rows = [];
	for (const [theme, selector] of [['light', ':root'], ['dark', '.dark']]) {
		const own = block(css, selector);
		const t = theme === 'dark' ? { ...block(css, ':root'), ...own } : own;
		for (const [pairs, bar, kind] of [[TEXT, 4.5, 'text'], [MARKS, 3, 'non-text']]) {
			for (const [fg, bg] of pairs) {
				const ground = tint(t, bg);
				if (!(fg in t) || ground === undefined) throw new Error(`${theme}: --${fg} or --${bg} is missing`);
				const r = ratio(t[fg], ground);
				rows.push({ theme, fg, bg, kind, bar, r, pass: r >= bar, fgHex: hex(t[fg]), bgHex: hex(ground) });
			}
		}
	}
	return rows;
}

export function markdown(rows) {
	const lines = [
		'# Contrast table',
		'',
		'Generated from `design/tokens.css` by `scripts/design-contrast.mjs`; CI fails if any pair misses its bar or',
		'this file is out of date (D-210). Text needs 4.5:1 (WCAG 1.4.3); field boundaries, focus rings, icons and chart',
		'marks need 3:1 (1.4.11).',
		'',
		'| Theme | Foreground | Background | Kind | Ratio | Bar | |',
		'|---|---|---|---|---:|---:|---|'
	];
	for (const r of rows) {
		lines.push(
			`| ${r.theme} | \`--${r.fg}\` ${r.fgHex} | \`--${r.bg}\` ${r.bgHex} | ${r.kind} | ${r.r.toFixed(2)}:1 | ${r.bar}:1 | ${r.pass ? 'pass' : '**FAIL**'} |`
		);
	}
	return lines.join('\n') + '\n';
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
	const rows = table(readFileSync(source, 'utf8'));
	const out = markdown(rows);
	const [flag, file] = process.argv.slice(2);
	if (flag === '--check-copy') {
		if (readFileSync(file, 'utf8') !== readFileSync(source, 'utf8')) {
			console.error(`${file} differs from design/tokens.css: copy it again (cp design/tokens.css ${file})`);
			process.exit(1);
		}
		console.log(`${file} is a byte copy of design/tokens.css`);
		process.exit(0);
	}
	if (flag === '--write') writeFileSync(file, out);
	if (flag === '--check' && readFileSync(file, 'utf8') !== out) {
		console.error(`${file} is out of date: run node scripts/design-contrast.mjs --write ${file}`);
		process.exit(1);
	}
	const failed = rows.filter((r) => !r.pass);
	for (const r of rows) console.log(`${r.pass ? 'pass' : 'FAIL'}  ${r.theme.padEnd(5)} --${r.fg} on --${r.bg}: ${r.r.toFixed(2)}:1 (bar ${r.bar})`);
	if (failed.length > 0) {
		console.error(`${failed.length} pair(s) below their bar`);
		process.exit(1);
	}
}
