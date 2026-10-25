// Material Design 3 colour tokens from one brand seed (docs/modules/14-ui.md, "Colour"): six
// schemes (light and dark, each at standard, medium and high contrast) as CSS custom
// properties. Pure: the generator script writes the result, and a test checks that the
// committed file still matches it.
//
// The fidelity scheme keeps the seed's hue and chroma for the primary role. The default
// tonal-spot scheme mutes it (#2f5bd3 came out #4c5c92), which suits wallpaper-derived themes,
// not a brand colour. Pinned to 0.3.0: 0.4.0 imports `./dynamic_color` without its extension,
// which Node refuses to load (checked 2026-10-26).

import {
	argbFromHex,
	Blend,
	DynamicColor,
	Hct,
	hexFromArgb,
	MaterialDynamicColors,
	SchemeFidelity,
	TonalPalette
} from '@material/material-color-utilities';

export interface Theme {
	seed: string;
	success: string;
}

export const CONTRAST_LEVELS = { standard: 0, medium: 0.5, high: 1 } as const;
export type ContrastName = keyof typeof CONTRAST_LEVELS;

/** The roles emitted, as CSS names. */
export const ROLES: Record<string, DynamicColor> = {
	primary: MaterialDynamicColors.primary,
	'on-primary': MaterialDynamicColors.onPrimary,
	'primary-container': MaterialDynamicColors.primaryContainer,
	'on-primary-container': MaterialDynamicColors.onPrimaryContainer,
	secondary: MaterialDynamicColors.secondary,
	'on-secondary': MaterialDynamicColors.onSecondary,
	'secondary-container': MaterialDynamicColors.secondaryContainer,
	'on-secondary-container': MaterialDynamicColors.onSecondaryContainer,
	tertiary: MaterialDynamicColors.tertiary,
	'on-tertiary': MaterialDynamicColors.onTertiary,
	'tertiary-container': MaterialDynamicColors.tertiaryContainer,
	'on-tertiary-container': MaterialDynamicColors.onTertiaryContainer,
	error: MaterialDynamicColors.error,
	'on-error': MaterialDynamicColors.onError,
	'error-container': MaterialDynamicColors.errorContainer,
	'on-error-container': MaterialDynamicColors.onErrorContainer,
	background: MaterialDynamicColors.background,
	'on-background': MaterialDynamicColors.onBackground,
	surface: MaterialDynamicColors.surface,
	'on-surface': MaterialDynamicColors.onSurface,
	'surface-variant': MaterialDynamicColors.surfaceVariant,
	'on-surface-variant': MaterialDynamicColors.onSurfaceVariant,
	'surface-dim': MaterialDynamicColors.surfaceDim,
	'surface-bright': MaterialDynamicColors.surfaceBright,
	'surface-container-lowest': MaterialDynamicColors.surfaceContainerLowest,
	'surface-container-low': MaterialDynamicColors.surfaceContainerLow,
	'surface-container': MaterialDynamicColors.surfaceContainer,
	'surface-container-high': MaterialDynamicColors.surfaceContainerHigh,
	'surface-container-highest': MaterialDynamicColors.surfaceContainerHighest,
	'inverse-surface': MaterialDynamicColors.inverseSurface,
	'inverse-on-surface': MaterialDynamicColors.inverseOnSurface,
	'inverse-primary': MaterialDynamicColors.inversePrimary,
	outline: MaterialDynamicColors.outline,
	'outline-variant': MaterialDynamicColors.outlineVariant,
	shadow: MaterialDynamicColors.shadow,
	scrim: MaterialDynamicColors.scrim
};

/**
 * Tones for the custom success role ("Verified at configured locations"), harmonized toward the
 * seed. The library's custom colours don't adapt to contrast levels, so the tones step apart
 * as contrast rises, the way the built-in roles do.
 */
const SUCCESS_TONES: Record<'light' | 'dark', Record<ContrastName, [number, number, number, number]>> = {
	light: { standard: [40, 100, 90, 10], medium: [30, 100, 40, 100], high: [20, 100, 30, 100] },
	dark: { standard: [80, 20, 30, 90], medium: [85, 10, 70, 0], high: [92, 0, 80, 0] }
};

/** One scheme: role name to hex. */
export function scheme(theme: Theme, dark: boolean, contrast: ContrastName): Record<string, string> {
	const source = Hct.fromInt(argbFromHex(theme.seed));
	const dynamic = new SchemeFidelity(source, dark, CONTRAST_LEVELS[contrast]);
	const colors: Record<string, string> = {};
	for (const [name, role] of Object.entries(ROLES)) colors[name] = hexFromArgb(role.getArgb(dynamic));

	// Harmonized toward the seed, and calmed: at full chroma the containers read as neon mint
	// next to the blue. Chroma 40 keeps it clearly green.
	const harmonized = Hct.fromInt(Blend.harmonize(argbFromHex(theme.success), argbFromHex(theme.seed)));
	const success = TonalPalette.fromHueAndChroma(harmonized.hue, Math.min(harmonized.chroma, 40));
	const [main, on, container, onContainer] = SUCCESS_TONES[dark ? 'dark' : 'light'][contrast];
	colors['success'] = hexFromArgb(success.tone(main));
	colors['on-success'] = hexFromArgb(success.tone(on));
	colors['success-container'] = hexFromArgb(success.tone(container));
	colors['on-success-container'] = hexFromArgb(success.tone(onContainer));
	return colors;
}

function block(selector: string, colors: Record<string, string>, indent = ''): string {
	const lines = Object.entries(colors).map(([name, hex]) => `${indent}\t--md-sys-color-${name}: ${hex};`);
	return `${indent}${selector} {\n${lines.join('\n')}\n${indent}}`;
}

/** The whole tokens.css. Light is the default; dark follows the system unless `data-theme`
 *  says otherwise; `data-contrast` picks medium or high contrast. */
export function tokensCss(theme: Theme): string {
	const parts = [
		`/* Generated by packages/ui/scripts/generate-tokens.ts from theme.json (seed ${theme.seed}). Do not edit:\n   change theme.json and run \`npm run generate -w @colophon/ui\`. */`,
		block(':root', scheme(theme, false, 'standard')),
		`@media (prefers-color-scheme: dark) {\n${block(':root:not([data-theme="light"])', scheme(theme, true, 'standard'), '\t')}\n}`,
		block(':root[data-theme="dark"]', scheme(theme, true, 'standard'))
	];
	for (const contrast of ['medium', 'high'] as const) {
		parts.push(
			block(`:root[data-contrast="${contrast}"]`, scheme(theme, false, contrast)),
			`@media (prefers-color-scheme: dark) {\n${block(`:root[data-contrast="${contrast}"]:not([data-theme="light"])`, scheme(theme, true, contrast), '\t')}\n}`,
			block(`:root[data-contrast="${contrast}"][data-theme="dark"]`, scheme(theme, true, contrast))
		);
	}
	return `${parts.join('\n\n')}\n`;
}
