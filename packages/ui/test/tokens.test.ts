// The design system's colours (D-182, D-210) keep their promises: every text colour reads on every
// ground it is used on, at WCAG AA, and at AAA when the system asks for more contrast; the
// accent's fill carries its label; status tags read in ink and in plain text; focus rings and
// the edges of checkboxes and radios stand out (3:1); and the stylesheet stays self-contained (no
// fonts or files from elsewhere).

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { contrast, type Oklch, parseOklch } from './contrast.ts';

// D-210: light and dark live in the token source's copy (tokens.css); the more-contrast overrides stay in styles.css.
const tokensCss = readFileSync(join(import.meta.dirname, '../src/lib/tokens.css'), 'utf8');
const stylesCss = readFileSync(join(import.meta.dirname, '../src/lib/styles.css'), 'utf8');

/** The declarations of the first block whose selector is exactly `selector`, after `from`. */
function block(selector: string, from = 0, css = stylesCss): Record<string, string> {
	const start = css.indexOf(`\n${selector} {`, from) + 1 || css.indexOf(`\t${selector} {`, from) + 1;
	if (start === 0) throw new Error(`no ${selector} block`);
	const body = css.slice(css.indexOf('{', start) + 1, css.indexOf('}', start));
	return Object.fromEntries(
		[...body.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(([, name = '', value = '']) => [name, value.trim()])
	);
}

const media = stylesCss.indexOf('@media (prefers-contrast: more)');
const light = block(':root', 0, tokensCss);
const dark = { ...light, ...block('.dark', 0, tokensCss) };
const lightMore = { ...light, ...block('\t:root:not(.dark)'.trim(), media) };
const darkMore = { ...dark, ...block('.dark', media) };

function colour(tokens: Record<string, string>, name: string): Oklch {
	let value = tokens[name];
	for (let hops = 0; value?.startsWith('var(') && hops < 5; hops += 1) value = tokens[value.slice(6, -1)];
	if (value === undefined) throw new Error(`--${name} is not defined`);
	return parseOklch(value);
}

const WHITE: Oklch = { l: 1, c: 0, h: 0 };
const GROUNDS = ['background', 'card', 'popover', 'muted', 'secondary', 'accent', 'sidebar', 'sidebar-accent'];
/** The status tones (D-182): each has a mark, a ring, a soft ground and an ink. */
const STATUSES = ['live', 'changed', 'pending', 'attention', 'draft'];
/** Each text colour and the grounds it is drawn on. */
const TEXT: [string, string[]][] = [
	// Status tags set their word in the plain ink, as the public site's tags do.
	['foreground', [...GROUNDS, 'brand-soft', ...STATUSES.map((status) => `${status}-soft`)]],
	['muted-foreground', ['background', 'card', 'popover', 'muted', 'sidebar']],
	['sidebar-foreground', ['sidebar', 'sidebar-accent']],
	// The one primary button, at rest and under the pointer.
	['primary-foreground', ['primary', 'primary-hover']],
	...STATUSES.map((status): [string, string[]] => [`${status}-ink`, [`${status}-soft`, 'card', 'background']]),
	// The accent as text (links, the selection's label): never the brighter --brand itself.
	['brand-ink', ['background', 'card', 'popover', 'sidebar', 'brand-soft']],
	['destructive', ['card', 'background', 'popover', 'muted', 'secondary']],
	['destructive-foreground', ['destructive']]
];
/** Marks that aren't text but must be seen (WCAG 1.4.11, 3:1): the focus ring and the edge of an
 *  unchecked checkbox or radio, on every ground they sit on. */
const EDGES: [string, string[]][] = [
	['ring', ['background', 'card', 'popover', 'sidebar']],
	['control', ['background', 'card', 'popover']]
];

const SCHEMES = [
	{ name: 'light', tokens: light, minimum: 4.5 },
	{ name: 'dark', tokens: dark, minimum: 4.5 },
	{ name: 'light, more contrast', tokens: lightMore, minimum: 7 },
	{ name: 'dark, more contrast', tokens: darkMore, minimum: 7 }
];

test('the high-contrast blocks exist and override what they should', () => {
	expect(media).toBeGreaterThan(0);
	expect(lightMore['muted-foreground']).not.toBe(light['muted-foreground']);
	expect(darkMore['muted-foreground']).not.toBe(dark['muted-foreground']);
});

for (const { name, tokens, minimum } of SCHEMES) {
	describe(`${name}: text reads on its grounds (at least ${minimum}:1)`, () => {
		for (const [text, grounds] of TEXT) {
			// The destructive colour marks errors beside words, and fills only a confirmation's
			// last button; it needs AA in every scheme.
			const needed = text.startsWith('destructive') ? 4.5 : minimum;
			for (const ground of grounds) {
				test(`--${text} on --${ground}`, () => {
					expect(contrast(colour(tokens, text), colour(tokens, ground))).toBeGreaterThanOrEqual(needed);
				});
			}
		}
	});
	for (const [edge, grounds] of EDGES) {
		test(`${name}: --${edge} stands out from what it sits on (3:1)`, () => {
			for (const ground of grounds) {
				expect(contrast(colour(tokens, edge), colour(tokens, ground)), ground).toBeGreaterThanOrEqual(3);
			}
		});
	}
}

test('a destructive button keeps white text readable', () => {
	expect(contrast(WHITE, colour(light, 'destructive'))).toBeGreaterThanOrEqual(4.5);
});

test('every status tone has its four tokens, in light and dark', () => {
	for (const status of STATUSES) {
		for (const part of ['', '-ring', '-soft', '-ink']) {
			expect(light[`${status}${part}`], `--${status}${part}`).toBeDefined();
			expect(block('.dark', 0, tokensCss)[`${status}${part}`], `dark --${status}${part}`).toBeDefined();
		}
	}
});

// D-210: the fill is the maintainer's approved deeper blue (hue 262.881) and the mark the pasted blue (259.903), so
// "one hue" means within 3 degrees: one blue to the eye, and still 30 degrees from every status (below).
test('the accent is one hue: its fill, mark, ink and ground agree within 3 degrees', () => {
	for (const tokens of [light, dark, lightMore, darkMore]) {
		const hues = ['primary', 'brand', 'brand-ink', 'brand-soft', 'link'].map((name) => colour(tokens, name).h);
		expect(Math.max(...hues) - Math.min(...hues)).toBeLessThanOrEqual(3);
	}
});

test('the accent is no status: its hue stays clear of every status hue', () => {
	const accent = colour(light, 'brand').h;
	for (const status of ['live', 'changed', 'pending', 'attention']) {
		const hue = colour(light, status).h;
		const apart = Math.min(Math.abs(accent - hue), 360 - Math.abs(accent - hue));
		expect(apart, status).toBeGreaterThanOrEqual(30);
	}
});

test('the stylesheet loads nothing from another origin', () => {
	for (const css of [stylesCss, tokensCss]) {
		expect(css).not.toMatch(/url\(\s*['"]?https?:/);
		expect(css).not.toMatch(/@import\s+['"]https?:/);
	}
});
