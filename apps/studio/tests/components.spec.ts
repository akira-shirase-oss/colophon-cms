// Shared components (D-073) in the Studio: which content types use one, wherever it sits (a field, a
// list's items, a section's fields); the field that inserts one; where it still fits, since its own
// structures count toward the three-deep limit; and the words for its refusals.

import { expect, test } from '@playwright/test';
import type { FieldDefinition } from '../src/lib/api.ts';
import { componentField, componentFits, usedBy } from '../src/lib/fieldDefs.ts';
import { INTERNAL_TERMS, INVALID_COMPONENT, UNKNOWN_COMPONENT } from '../src/lib/words.ts';

const title = { id: 'title', label: 'Title', required: false, type: 'text', max_chars: 200, multiline: false } as FieldDefinition;
const seo = { slug: 'seo', definition: { name: 'SEO', fields: [title] } };
const type = (slug: string, fields: FieldDefinition[]) => ({ slug, definition: { name: slug.toUpperCase(), fields } });

test('a component is used by the content types that hold it anywhere, and only those', () => {
	const asField = type('pages', [title, { id: 'seo', label: 'SEO', required: false, type: 'object', component: 'seo', fields: [title] }]);
	const asItem = type('posts', [{ id: 'links', label: 'Links', required: false, type: 'list', max: 5, of: { type: 'object', component: 'seo', fields: [title] } }]);
	const inSection = type('landing', [
		{ id: 'sections', label: 'Sections', required: false, type: 'blocks', max: 10, blocks: [{ name: 'hero', label: 'Hero', fields: [{ id: 'seo', label: 'SEO', required: false, type: 'object', component: 'seo', fields: [title] }] }] }
	]);
	const deep = type('docs', [{ id: 'meta', label: 'Meta', required: false, type: 'object', fields: [{ id: 'seo', label: 'SEO', required: false, type: 'object', component: 'seo', fields: [title] }] }]);
	const other = type('people', [title, { id: 'cta', label: 'CTA', required: false, type: 'object', component: 'cta', fields: [title] }]);
	const plainGroup = type('notes', [{ id: 'seo', label: 'SEO', required: false, type: 'object', fields: [title] }]);
	expect(usedBy([asField, asItem, inSection, deep, other, plainGroup], 'seo')).toEqual([
		{ slug: 'pages', name: 'PAGES' },
		{ slug: 'posts', name: 'POSTS' },
		{ slug: 'landing', name: 'LANDING' },
		{ slug: 'docs', name: 'DOCS' }
	]);
	expect(usedBy([asField], 'cta')).toEqual([]);
});

test('the field that inserts a component is a group marked with it, holding a copy of its fields', () => {
	const field = componentField('meta', 'Meta', seo);
	expect(field).toEqual({ id: 'meta', label: 'Meta', required: false, type: 'object', component: 'seo', fields: [title] });
	// A copy: editing the type's draft never edits the component.
	if (field.type === 'object') field.fields.push({ ...title, id: 'other' });
	expect(seo.definition.fields).toHaveLength(1);
});

test('a component fits where its own structures still nest at most three deep', () => {
	expect(componentFits(seo, 0)).toBe(true);
	expect(componentFits(seo, 2)).toBe(true);
	expect(componentFits(seo, 3)).toBe(false);
	const group = { slug: 'card', definition: { name: 'Card', fields: [{ id: 'cta', label: 'CTA', required: false, type: 'object', fields: [title] } as FieldDefinition] } };
	expect(componentFits(group, 1)).toBe(true);
	expect(componentFits(group, 2)).toBe(false);
	// A list of groups needs two levels: its items sit one deeper than the list.
	const listOfGroups = { slug: 'faq', definition: { name: 'FAQ', fields: [{ id: 'items', label: 'Items', required: false, type: 'list', max: 5, of: { type: 'object', fields: [title] } } as FieldDefinition] } };
	expect(componentFits(listOfGroups, 0)).toBe(true);
	expect(componentFits(listOfGroups, 1)).toBe(false);
});

test('the words are plain', () => {
	for (const words of [UNKNOWN_COMPONENT, INVALID_COMPONENT]) expect(INTERNAL_TERMS.test(words), words).toBe(false);
});
