// The bundle's own contract, checked without a server: the rules the domain enforces on parse
// (depth 3, twenty block kinds, field-id shape, required single-line title fields) plus the
// bundle's own conventions (every section kind carries the section-style component, defaults
// fit their types). The real proof is the install spec against the server.

import { describe, expect, it } from 'vitest';
import {
	HOME_PAGE,
	navigationSeed,
	PAGES_TYPE,
	SECTION_KINDS,
	SECTION_STYLE_COMPONENT,
	SITE_COMPONENTS,
	SITE_TYPES,
	THEME_SEED,
	THEME_TYPE,
	NAVIGATION_TYPE,
	type Field,
	type TypeDef
} from '../src/index.ts';

const FIELD_ID = /^[a-z][a-z0-9_]{0,63}$/;
const COLOR = /^#[0-9a-f]{6}$/;

/** How deep a field list nests: a group, a list and page sections each add one level. */
const depth = (fields: Field[], level = 0): number =>
	Math.max(
		...fields.map((field) => {
			if (field.type === 'object') return depth((field.fields as Field[]) ?? [], level + 1);
			if (field.type === 'blocks')
				return Math.max(...(field.blocks as { fields: Field[] }[]).map((kind) => depth(kind.fields, level + 1)));
			if (field.type === 'list') {
				const of = field.of as { type: string; fields?: Field[] };
				return of.type === 'object' ? depth(of.fields ?? [], level + 2) : level + 1;
			}
			return level;
		})
	);

const fieldIds = (fields: Field[], prefix = ''): { id: string; at: string }[] =>
	fields.flatMap((field) => {
		const at = `${prefix}${field.id}`;
		const nested =
			field.type === 'object'
				? fieldIds((field.fields as Field[]) ?? [], `${at}.`)
				: field.type === 'blocks'
					? (field.blocks as { name: string; fields: Field[] }[]).flatMap((kind) => fieldIds(kind.fields, `${at}.${kind.name}.`))
					: field.type === 'list' && (field.of as { type: string }).type === 'object'
						? fieldIds(((field.of as { fields: Field[] }).fields as Field[]) ?? [], `${at}.`)
						: [];
		return [{ id: field.id, at }, ...nested];
	});

const defaults = (fields: Field[]): { field: Field; value: unknown }[] =>
	fields.flatMap((field) => {
		const nested =
			field.type === 'object'
				? defaults((field.fields as Field[]) ?? [])
				: field.type === 'blocks'
					? (field.blocks as { fields: Field[] }[]).flatMap((kind) => defaults(kind.fields))
					: field.type === 'list' && (field.of as { type: string }).type === 'object'
						? defaults(((field.of as { fields?: Field[] }).fields as Field[]) ?? [])
						: [];
		return [...(field.default === undefined ? [] : [{ field, value: field.default }]), ...nested];
	});

describe('the bundle', () => {
	it('installs the component first, then its three types', () => {
		expect(SITE_COMPONENTS.map((item) => item.slug)).toEqual(['section-style']);
		expect(SITE_TYPES.map((item) => item.slug)).toEqual(['pages', 'navigation', 'theme']);
	});

	it('uses only well-formed field ids, with no repeats in a list', () => {
		for (const type of SITE_TYPES) {
			const top = type.definition.fields.map((field) => field.id);
			expect(new Set(top).size, `${type.slug} top-level ids`).toBe(top.length);
			for (const { id, at } of fieldIds(type.definition.fields)) expect(FIELD_ID.test(id), `${type.slug}: ${at}`).toBe(true);
		}
		for (const { id } of fieldIds(SECTION_STYLE_COMPONENT.definition.fields)) expect(FIELD_ID.test(id)).toBe(true);
	});

	it('stays within the modelling limits', () => {
		for (const type of SITE_TYPES) {
			expect(depth(type.definition.fields), `${type.slug} depth`).toBeLessThanOrEqual(3);
			expect(type.definition.fields.length, `${type.slug} fields`).toBeLessThanOrEqual(100);
		}
		const sections = PAGES_TYPE.definition.fields.find((field) => field.id === 'sections');
		expect(sections?.type).toBe('blocks');
		expect((sections?.blocks as unknown[]).length).toBeLessThanOrEqual(20);
		expect(new Set(SECTION_KINDS.map((kind) => kind.name)).size).toBe(SECTION_KINDS.length);
	});

	it('gives each type a required single-line text title field', () => {
		for (const type of SITE_TYPES) {
			const title = type.definition.fields.find((field) => field.id === type.definition.title_field) as Field & { multiline?: boolean };
			expect(title?.type, type.slug).toBe('text');
			expect(title?.required, type.slug).toBe(true);
			expect(title?.multiline, type.slug).toBe(false);
		}
	});

	it('marks the singles single and the collection not', () => {
		expect(PAGES_TYPE.definition.single).toBeUndefined();
		expect(NAVIGATION_TYPE.definition.single).toBe(true);
		expect(THEME_TYPE.definition.single).toBe(true);
	});

	it('offers the section-style options on every kind of section', () => {
		for (const kind of SECTION_KINDS) {
			const appearance = kind.fields.find((field) => field.id === 'style');
			expect(appearance?.component, kind.name).toBe('section-style');
			expect(appearance?.fields as Field[], kind.name).toEqual(SECTION_STYLE_COMPONENT.definition.fields);
		}
	});

	it('writes defaults that fit their fields', () => {
		for (const { field, value } of defaults([...SITE_TYPES.flatMap((type) => type.definition.fields), ...SECTION_STYLE_COMPONENT.definition.fields])) {
			if (field.type === 'enum') expect((field.values as string[]).includes(value as string), `${field.id}: ${String(value)}`).toBe(true);
			if (field.type === 'color') expect(COLOR.test(value as string), `${field.id}: ${String(value)}`).toBe(true);
			if (field.type === 'number') {
				expect(value as number, field.id).toBeGreaterThanOrEqual(field.min as number);
				expect(value as number, field.id).toBeLessThanOrEqual(field.max as number);
			}
		}
	});

	it('seeds content in definition order with _block first', () => {
		expect(Object.keys(HOME_PAGE)).toEqual(['title', 'slug', 'sections']);
		for (const item of HOME_PAGE.sections as Record<string, unknown>[]) {
			expect(Object.keys(item)[0]).toBe('_block');
			expect(SECTION_KINDS.some((kind) => kind.name === item._block), String(item._block)).toBe(true);
		}
		expect(navigationSeed('entry-id')).toMatchObject({ items: [{ label: 'Home', page: 'entry-id' }] });
		const light = THEME_SEED.light as Record<string, unknown>;
		const mode = (THEME_TYPE.definition.fields.find((field) => field.id === 'light')?.fields ?? []) as Field[];
		expect(Object.keys(light ?? {})).toEqual(mode.map((field) => field.id));
	});
});
