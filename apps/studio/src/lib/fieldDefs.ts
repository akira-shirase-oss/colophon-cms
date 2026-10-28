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

/** A section kind from its label. */
export const blankBlock = (label: string): BlockKind => ({ name: idFrom(label), label, fields: [] });
