import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import { type ContrastName, scheme, type Theme, tokensCss } from '../scripts/tokens.ts';
import { contrast } from './contrast.ts';

const root = join(import.meta.dirname, '..');
const theme = JSON.parse(readFileSync(join(root, 'theme.json'), 'utf8')) as Theme;

test('the committed tokens.css is what theme.json generates', () => {
	expect(readFileSync(join(root, 'src/tokens.css'), 'utf8')).toBe(tokensCss(theme));
});

/** Each role and the text drawn on it. */
const PAIRS: [string, string][] = [
	['primary', 'on-primary'],
	['primary-container', 'on-primary-container'],
	['secondary', 'on-secondary'],
	['secondary-container', 'on-secondary-container'],
	['tertiary', 'on-tertiary'],
	['tertiary-container', 'on-tertiary-container'],
	['error', 'on-error'],
	['error-container', 'on-error-container'],
	['success', 'on-success'],
	['success-container', 'on-success-container'],
	['background', 'on-background'],
	['surface', 'on-surface'],
	['surface-container', 'on-surface'],
	['surface-container-highest', 'on-surface'],
	['surface', 'on-surface-variant'],
	['surface-container-highest', 'on-surface-variant'],
	['inverse-surface', 'inverse-on-surface'],
	['surface', 'primary'],
	['surface', 'error']
];

/** WCAG AA for text (4.5:1) at standard contrast; AAA (7:1) at high contrast. */
const MINIMUM: Record<ContrastName, number> = { standard: 4.5, medium: 4.5, high: 7 };

describe('every role keeps enough contrast with the text on it', () => {
	for (const dark of [false, true]) {
		for (const level of ['standard', 'medium', 'high'] as const) {
			const colors = scheme(theme, dark, level);
			test.each(PAIRS)(`${dark ? 'dark' : 'light'} ${level}: %s / %s`, (background, foreground) => {
				const ratio = contrast(colors[background] as string, colors[foreground] as string);
				expect(ratio, `${background} ${colors[background]} vs ${foreground} ${colors[foreground]}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(MINIMUM[level]);
			});
		}
	}
});

test('the contrast function matches known WCAG values', () => {
	expect(contrast('#000000', '#ffffff')).toBeCloseTo(21, 5);
	expect(contrast('#777777', '#ffffff')).toBeCloseTo(4.48, 2);
});
