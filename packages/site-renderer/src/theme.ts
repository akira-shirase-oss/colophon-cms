// The theme entry's content to CSS custom properties (D-089): the fixed token set the built-in
// sections consume, plus the entry's own `tokens` list emitted as --site-x-<name>. Values come
// from validated content, but the renderer checks them again at the seam — a colour that is not
// #rrggbb, a token name outside its pattern, or a token value that could break out of a
// declaration (a `;` or a brace) is refused rather than emitted.

import { UnrenderableSite } from './errors.ts';

export interface ThemeMode {
	primary?: unknown;
	secondary?: unknown;
	accent?: unknown;
	surface?: unknown;
	text?: unknown;
	muted?: unknown;
	radius_sm?: unknown;
	radius_md?: unknown;
	radius_lg?: unknown;
	shadow?: unknown;
	font_body?: unknown;
	font_heading?: unknown;
	base_size?: unknown;
}

export interface ThemeToken {
	name?: unknown;
	value?: unknown;
}

/** The theme entry's `content` field. `light` is required by the schema; the rest may be absent. */
export interface ThemeContent {
	light?: ThemeMode;
	dark?: ThemeMode;
	tokens?: ThemeToken[];
}

const FONT_STACKS: Record<string, string> = {
	system: 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
	sans: 'ui-sans-serif, "Helvetica Neue", Arial, sans-serif',
	serif: 'ui-serif, Georgia, "Times New Roman", serif',
	mono: 'ui-monospace, "SF Mono", Menlo, Consolas, monospace'
};

const SHADOWS: Record<string, string> = {
	none: 'none',
	subtle: '0 1px 2px rgb(0 0 0 / 0.06), 0 1px 3px rgb(0 0 0 / 0.08)',
	raised: '0 4px 12px rgb(0 0 0 / 0.12)',
	floating: '0 12px 32px rgb(0 0 0 / 0.16)'
};

function colour(value: unknown, name: string): string {
	if (typeof value !== 'string' || !/^#[0-9a-f]{6}$/i.test(value)) throw new UnrenderableSite(`theme ${name} is not a #rrggbb colour`);
	return value.toLowerCase();
}

function pixels(value: unknown, name: string): string {
	if (typeof value !== 'number' || !Number.isInteger(value) || value < 0) throw new UnrenderableSite(`theme ${name} is not a size in pixels`);
	return `${value}px`;
}

function preset(value: unknown, table: Record<string, string>, name: string): string {
	if (typeof value !== 'string' || !(value in table)) throw new UnrenderableSite(`theme ${name} is not one it knows`);
	return table[value] as string;
}

/** One mode's tokens as CSS custom properties, without a selector. */
export function modeVars(mode: ThemeMode): Record<string, string> {
	const vars: Record<string, string> = {};
	if (mode.primary !== undefined) vars['--site-primary'] = colour(mode.primary, 'primary');
	if (mode.secondary !== undefined) vars['--site-secondary'] = colour(mode.secondary, 'secondary');
	if (mode.accent !== undefined) vars['--site-accent'] = colour(mode.accent, 'accent');
	if (mode.surface !== undefined) vars['--site-surface'] = colour(mode.surface, 'surface');
	if (mode.text !== undefined) vars['--site-text'] = colour(mode.text, 'text');
	if (mode.muted !== undefined) vars['--site-muted'] = colour(mode.muted, 'muted');
	if (mode.radius_sm !== undefined) vars['--site-radius-sm'] = pixels(mode.radius_sm, 'radius_sm');
	if (mode.radius_md !== undefined) vars['--site-radius-md'] = pixels(mode.radius_md, 'radius_md');
	if (mode.radius_lg !== undefined) vars['--site-radius-lg'] = pixels(mode.radius_lg, 'radius_lg');
	if (mode.shadow !== undefined) vars['--site-shadow'] = preset(mode.shadow, SHADOWS, 'shadow');
	if (mode.font_body !== undefined) vars['--site-font-body'] = preset(mode.font_body, FONT_STACKS, 'font_body');
	if (mode.font_heading !== undefined) vars['--site-font-heading'] = preset(mode.font_heading, FONT_STACKS, 'font_heading');
	if (mode.base_size !== undefined) vars['--site-base-size'] = pixels(mode.base_size, 'base_size');
	return vars;
}

/** The entry's custom tokens as `--site-x-<name>` properties. */
export function tokenVars(tokens: ThemeToken[] | undefined): Record<string, string> {
	const vars: Record<string, string> = {};
	for (const token of tokens ?? []) {
		if (typeof token.name !== 'string' || !/^[a-z][a-z0-9_]{0,31}$/.test(token.name)) throw new UnrenderableSite('a custom token name is not its pattern');
		if (typeof token.value !== 'string' || /[;{}<>]/.test(token.value)) throw new UnrenderableSite(`custom token ${token.name} could break out of its declaration`);
		vars[`--site-x-${token.name}`] = token.value;
	}
	return vars;
}

/** The declarations of a vars map as `name:value;…` — the inner text of one ruleset. */
export function declarations(vars: Record<string, string>): string {
	return Object.entries(vars)
		.map(([name, value]) => `${name}:${value};`)
		.join('');
}

/** The theme entry's whole stylesheet fragment: light tokens on :root, the dark palette under
 *  prefers-color-scheme when the entry has one, custom tokens alongside light. */
export function themeCss(theme: ThemeContent): string {
	if (theme.light === undefined) throw new UnrenderableSite('the theme has no light mode');
	const light = declarations({ ...modeVars(theme.light), ...tokenVars(theme.tokens) });
	const dark = theme.dark === undefined ? '' : `@media (prefers-color-scheme:dark){:root{${declarations(modeVars(theme.dark))}}}`;
	return `:root{${light}}${dark}`;
}
