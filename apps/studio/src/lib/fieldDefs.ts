// The content-type builder's vocabulary and rules (D-040, D-054, D-064, D-065, D-066): what each
// field type is called, a new field of each type, and the limits the server keeps, so the builder
// only offers what a save would accept and says why when it stops.

import { type BlockKind, type FieldDefinition, type FieldType, type ItemKind, MAX_DEPTH } from './api.ts';

/** Each field type in plain words (D-059). */
export const NAMES: Record<FieldType, string> = {
	text: 'Text',
	rich_text: 'Rich text',
	markdown: 'Markdown text',
	number: 'Number',
	boolean: 'Yes or no',
	enum: 'Choice',
	datetime: 'Date and time',
	date: 'Date',
	time: 'Time of day',
	email: 'Email address',
	url: 'Web address',
	color: 'Colour',
	json: 'Structured data (JSON)',
	reference: 'Link to entries',
	object: 'Group of fields',
	list: 'List',
	blocks: 'Page sections',
	slug: 'URL name'
};

/** What each kind of field is for, in one plain line: the add-a-field picker's cards. */
export const ABOUT: Record<FieldType | 'component', string> = {
	text: 'A title, a name, a short description.',
	rich_text: 'Long writing with headings, lists, links and images.',
	markdown: 'Text written in Markdown, for people who know it.',
	number: 'A price, a quantity, a rating.',
	boolean: 'A switch: yes or no, on or off.',
	enum: 'One option from a list you set, like a category.',
	datetime: 'A day and a time, like when an event starts.',
	date: 'A day, like a birthday or a launch date.',
	time: 'A time of day, like when a shop opens.',
	email: 'An email address, checked as one.',
	url: 'A link to a page on the web.',
	color: 'A colour, like a brand accent.',
	json: 'Any structured data, for developers.',
	reference: 'Points to entries of another type, like an author.',
	object: 'Fields kept together, like an address.',
	list: 'Several values of one kind, like tags or questions.',
	blocks: 'Pick and order blocks while writing, like a page builder.',
	slug: 'The part of an address that names an entry, like lemon-tart.',
	component: 'A group of fields shared between content types, like SEO.'
};

/** The add-a-field picker's groups, in order. Every field type is in one. */
export const PICKER_GROUPS: { name: string; kinds: (FieldType | 'component')[] }[] = [
	{ name: 'Basics', kinds: ['text', 'rich_text', 'number', 'boolean', 'enum', 'datetime', 'date', 'time'] },
	{ name: 'Links', kinds: ['email', 'url'] },
	{ name: 'Structure', kinds: ['reference', 'object', 'list', 'blocks', 'component'] },
	{ name: 'Advanced', kinds: ['markdown', 'json', 'color', 'slug'] }
];

/** A code name (a field's id): a lowercase letter, then lowercase letters, digits and underscores,
 *  64 at most, as the server requires. */
export const CODE_NAME = /^[a-z][a-z0-9_]{0,63}$/;

/** A field's kind in plain words, naming the shared component or what a list holds. */
export function describeField(field: FieldDefinition | ItemKind, componentName: (slug: string) => string): string {
	if (field.type === 'object' && field.component !== undefined) return `Component: ${componentName(field.component)}`;
	if (field.type === 'list') return `List of ${describeField(field.of, componentName).toLowerCase()}`;
	return NAMES[field.type];
}

/** Where a refusal's JSON Pointer lands for the field at `at` ("/fields/2"): on that field itself
 *  ("" for the whole field, or the option: "/pattern", "/of/pattern"), or null when it's elsewhere,
 *  including deeper inside the field's own fields, whose rows say it. */
export function refusalAt(pointer: string, at: string): string | null {
	if (pointer !== at && !pointer.startsWith(`${at}/`)) return null;
	const rest = pointer.slice(at.length);
	return /^\/(fields|blocks|of\/fields)\//.test(rest) ? null : rest;
}

/** The structures, which each take one level of nesting. */
export const STRUCTURES: FieldType[] = ['object', 'list', 'blocks'];

/** What a list's items can be: any field kind but a list, page sections or a URL name. */
export const ITEM_TYPES: FieldType[] = ['text', 'markdown', 'rich_text', 'number', 'boolean', 'enum', 'datetime', 'date', 'time', 'email', 'url', 'color', 'json', 'reference', 'object'];

/** A field id (or a section kind's name) from a label: lowercase letters, digits and underscores,
 *  starting with a letter. */
export function idFrom(label: string): string {
	const id = label
		.normalize('NFKD')
		.replace(/\p{M}+/gu, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '_')
		.replace(/^[^a-z]+|_+$/g, '');
	return id.slice(0, 64);
}

/** A new item kind of `type`, with the server's defaults. */
export function blankKind(type: FieldType, target: string): ItemKind | FieldDefinition {
	switch (type) {
		case 'text':
			return { type: 'text', max_chars: 200, multiline: false };
		case 'number':
			return { type: 'number', integer: false };
		case 'enum':
			return { type: 'enum', values: ['First option'] };
		case 'markdown':
			return { type: 'markdown', max_chars: 100_000 };
		case 'reference':
			return { type: 'reference', to: target, many: false };
		case 'object':
			return { type: 'object', fields: [] };
		case 'list':
			// A field's kind (`blank` adds its id, label and flags).
			return { type: 'list', of: { type: 'text', max_chars: 200, multiline: false }, max: 10 } as unknown as ItemKind;
		case 'blocks':
			return { type: 'blocks', blocks: [], max: 20 } as unknown as ItemKind;
		default:
			return { type } as ItemKind;
	}
}

/** A new field of `type`. `target` is where a new link points first; `source` a URL name's text. */
export function blank(type: FieldType, id: string, label: string, target: string, source: string): FieldDefinition {
	const common = { id, label, required: false };
	if (type === 'slug') return { ...common, type: 'slug', source };
	return { ...common, ...blankKind(type, target) } as FieldDefinition;
}

/** How many structure levels a field of `type` at `depth` uses for its own fields: a group's and
 *  a section kind's fields sit one deeper; a list's item one deeper, and an item group's fields two. */
export const canNest = (depth: number) => depth < MAX_DEPTH;

/** What keeps a definition from being saved, found before the server says it: a group or a kind of
 *  section with no fields, page sections with no kinds. One sentence each, with where. */
export function structureProblems(fields: FieldDefinition[], within = ''): string[] {
	const problems: string[] = [];
	for (const field of fields) {
		const where = within === '' ? field.label : `${within} → ${field.label}`;
		if (field.type === 'object') {
			if (field.fields.length === 0) problems.push(`${where} needs at least one field.`);
			problems.push(...structureProblems(field.fields, where));
		} else if (field.type === 'list' && field.of.type === 'object') {
			if (field.of.fields.length === 0) problems.push(`Each item of ${where} needs at least one field.`);
			problems.push(...structureProblems(field.of.fields, `${where} → item`));
		} else if (field.type === 'blocks') {
			if (field.blocks.length === 0) problems.push(`${where} needs at least one kind of section.`);
			for (const kind of field.blocks) {
				if (kind.fields.length === 0) problems.push(`The ${kind.label} section of ${where} needs at least one field.`);
				problems.push(...structureProblems(kind.fields, `${where} → ${kind.label}`));
			}
		}
	}
	return problems;
}

/** Every saved field's path ("hero/title", "sections/quote/by", "faq/item/q"), so the builder knows
 *  which are new: a saved link keeps pointing where it points. */
export function savedPaths(fields: FieldDefinition[], within = '', out = new Set<string>()): Set<string> {
	for (const field of fields) {
		const path = within === '' ? field.id : `${within}/${field.id}`;
		out.add(path);
		if (field.type === 'object') savedPaths(field.fields, path, out);
		if (field.type === 'list' && field.of.type === 'object') savedPaths(field.of.fields, `${path}/item`, out);
		if (field.type === 'blocks') for (const kind of field.blocks) savedPaths(kind.fields, `${path}/${kind.name}`, out);
	}
	return out;
}

/** Whether a field may be unique (D-070): a top-level single-line text, email, web address,
 *  number, date, time or date-and-time field. */
export function canBeUnique(field: FieldDefinition, depth: number): boolean {
	if (depth !== 0) return false;
	if (field.type === 'text') return !field.multiline;
	return ['email', 'url', 'number', 'date', 'time', 'datetime'].includes(field.type);
}

/** Whether a field may have a default (D-071): top-level, not unique, and of a kind with one sensible
 *  starting value (not rich text, JSON, links, URL names or structures). */
export function canHaveDefault(field: FieldDefinition, depth: number): boolean {
	return (
		depth === 0 &&
		field.unique !== true &&
		['text', 'markdown', 'number', 'boolean', 'enum', 'date', 'time', 'datetime', 'email', 'url', 'color'].includes(field.type)
	);
}

/** The control a field's default is chosen with in the builder: the field's own kind of input. */
export type DefaultInput = 'text' | 'textarea' | 'number' | 'yes-no' | 'choice' | 'date' | 'time' | 'datetime-local' | 'email' | 'url' | 'color';

export function defaultInput(field: FieldDefinition): DefaultInput | null {
	switch (field.type) {
		case 'text':
			return field.multiline ? 'textarea' : 'text';
		case 'markdown':
			return 'textarea';
		case 'number':
			return 'number';
		case 'boolean':
			return 'yes-no';
		case 'enum':
			return 'choice';
		case 'date':
			return 'date';
		case 'time':
			return 'time';
		case 'datetime':
			return 'datetime-local';
		case 'email':
			return 'email';
		case 'url':
			return 'url';
		case 'color':
			return 'color';
		default:
			return null;
	}
}

/** The content types that use the component `slug`, anywhere: as a field, a list's items or a
 *  section's field (D-073). */
export function usedBy(types: { slug: string; definition: { name: string; fields: FieldDefinition[] } }[], slug: string): { slug: string; name: string }[] {
	const uses = (fields: FieldDefinition[]): boolean =>
		fields.some(
			(field) =>
				(field.type === 'object' && (field.component === slug || uses(field.fields))) ||
				(field.type === 'list' && field.of.type === 'object' && (field.of.component === slug || uses(field.of.fields))) ||
				(field.type === 'blocks' && field.blocks.some((kind) => uses(kind.fields)))
		);
	return types.filter((type) => uses(type.definition.fields)).map((type) => ({ slug: type.slug, name: type.definition.name }));
}

/** How many structure levels a field needs below its own depth (D-066): one for a structure, plus
 *  what its fields need; a list's item sits one deeper, so a list of groups needs two. */
function levels(field: FieldDefinition | ItemKind): number {
	if (field.type === 'object') return 1 + headroom(field.fields);
	if (field.type === 'list') return 1 + levels(field.of);
	if (field.type === 'blocks') return 1 + Math.max(0, ...field.blocks.map((kind) => headroom(kind.fields)));
	return 0;
}
const headroom = (fields: FieldDefinition[]) => Math.max(0, ...fields.map(levels));

/** Whether a component fits as a group of fields at `depth`: its own structures still nest at most
 *  three deep there. A list's item sits one deeper than the list. */
export const componentFits = (component: { definition: { fields: FieldDefinition[] } }, depth: number) =>
	canNest(depth + headroom(component.definition.fields));

/** A field using a component: an object marked with its slug, holding its fields (the server copies
 *  the current ones in, whatever is sent). */
export function componentField(id: string, label: string, component: { slug: string; definition: { fields: FieldDefinition[] } }): FieldDefinition {
	return { id, label, required: false, type: 'object', component: component.slug, fields: structuredClone(component.definition.fields) };
}

/** A section kind from its label. */
export const blankBlock = (label: string): BlockKind => ({ name: idFrom(label), label, fields: [] });
