// The design system's colours (D-053) keep their promises: every text colour reads on every
// ground it is used on, at WCAG AA, and at AAA when the system asks for more contrast; focus
// rings stand out; and the stylesheet stays self-contained (no fonts or files from elsewhere).

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { contrast, type Oklch, parseOklch } from './contrast.ts';

const css = readFileSync(join(import.meta.dirname, '../src/lib/styles.css'), 'utf8');

/** The declarations of the first block whose selector is exactly `selector`, after `from`. */
function block(selector: string, from = 0): Record<string, string> {
	const start = css.indexOf(`\n${selector} {`, from) + 1 || css.indexOf(`\t${selector} {`, from) + 1;
	if (start === 0) throw new Error(`no ${selector} block`);
	const body = css.slice(css.indexOf('{', start) + 1, css.indexOf('}', start));
	return Object.fromEntries(
		[...body.matchAll(/--([\w-]+):\s*([^;]+);/g)].map(([, name = '', value = '']) => [name, value.trim()])
	);
}

const media = css.indexOf('@media (prefers-contrast: more)');
const light = block(':root');
const dark = { ...light, ...block('.dark') };
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
/** Each text colour and the grounds it is drawn on. */
const TEXT: [string, string[]][] = [
	['foreground', GROUNDS],
	['muted-foreground', ['background', 'card', 'popover', 'muted', 'sidebar']],
	['sidebar-foreground', ['sidebar', 'sidebar-accent']],
	['primary-foreground', ['primary']],
	['live-ink', ['live-soft', 'card']],
	['pending-ink', ['pending-soft', 'card']],
	['attention-ink', ['attention-soft', 'card']],
	['draft-ink', ['draft-soft', 'card']],
	['destructive', ['card', 'background']]
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
			// The destructive colour marks errors beside words; it needs AA in every scheme.
			const needed = text === 'destructive' ? 4.5 : minimum;
			for (const ground of grounds) {
				test(`--${text} on --${ground}`, () => {
					expect(contrast(colour(tokens, text), colour(tokens, ground))).toBeGreaterThanOrEqual(needed);
				});
			}
		}
	});
	test(`${name}: the focus ring stands out from the page (3:1)`, () => {
		for (const ground of ['background', 'card', 'sidebar']) {
			expect(contrast(colour(tokens, 'ring'), colour(tokens, ground)), ground).toBeGreaterThanOrEqual(3);
		}
	});
}

test('a destructive button keeps white text readable', () => {
	expect(contrast(WHITE, colour(light, 'destructive'))).toBeGreaterThanOrEqual(4.5);
});

test('the stylesheet loads nothing from another origin', () => {
	expect(css).not.toMatch(/url\(\s*['"]?https?:/);
	expect(css).not.toMatch(/@import\s+['"]https?:/);
});
