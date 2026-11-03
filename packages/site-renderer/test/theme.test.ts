import { describe, expect, it } from 'vitest';
import { declarations, modeVars, themeCss, tokenVars, UnrenderableSite } from '../src/index.ts';
import { DEFAULT_MODE } from '@colophon/site-template';

describe('theme tokens to CSS variables', () => {
	it('maps the starter palette to --site-* properties', () => {
		const vars = modeVars(DEFAULT_MODE);
		expect(vars['--site-primary']).toBe('#6d28d9');
		expect(vars['--site-text']).toBe('#18181b');
		expect(vars['--site-radius-md']).toBe('8px');
		expect(vars['--site-base-size']).toBe('16px');
		expect(vars['--site-font-body']).toContain('system-ui');
		expect(vars['--site-shadow']).toContain('rgb(0 0 0 / 0.06)');
	});

	it('refuses a colour that is not #rrggbb', () => {
		expect(() => modeVars({ primary: 'rebeccapurple' })).toThrow(UnrenderableSite);
		expect(() => modeVars({ primary: '#fff' })).toThrow(UnrenderableSite);
		expect(() => modeVars({ primary: 'url(evil)' })).toThrow(UnrenderableSite);
	});

	it('refuses a shadow or font outside the presets', () => {
		expect(() => modeVars({ shadow: '0 0 999px red' })).toThrow(UnrenderableSite);
		expect(() => modeVars({ font_body: 'Papyrus' })).toThrow(UnrenderableSite);
	});

	it('emits a light :root block plus a dark media block when the entry has one', () => {
		const css = themeCss({ light: { primary: '#112233' }, dark: { primary: '#445566' } });
		expect(css).toContain(':root{--site-primary:#112233;}');
		expect(css).toContain('@media (prefers-color-scheme:dark){:root{--site-primary:#445566;}}');
	});

	it('omits the dark block when the entry has none', () => {
		expect(themeCss({ light: DEFAULT_MODE })).not.toContain('prefers-color-scheme');
	});

	it('requires the light mode', () => {
		expect(() => themeCss({})).toThrow(UnrenderableSite);
	});

	it('emits custom tokens as --site-x-* and refuses ones that break out', () => {
		expect(tokenVars([{ name: 'hero_size', value: '3rem' }])).toEqual({ '--site-x-hero_size': '3rem' });
		expect(() => tokenVars([{ name: 'Hero', value: '3rem' }])).toThrow(UnrenderableSite);
		expect(() => tokenVars([{ name: 'x', value: 'red;position:fixed' }])).toThrow(UnrenderableSite);
		expect(() => tokenVars([{ name: 'x', value: 'url(<bad>)' }])).toThrow(UnrenderableSite);
	});

	it('writes declarations in name:value; form', () => {
		expect(declarations({ '--site-a': '1', '--site-b': '2' })).toBe('--site-a:1;--site-b:2;');
	});
});
