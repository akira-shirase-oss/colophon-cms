// Content and its content type: what the editor needs to build, compare and show an entry
// generically, whatever fields its type has (D-040).

import { type RichTextValue, serialize } from '@colophon/editor';
import type { Content, Definition, FieldDefinition } from './api.ts';

export const EMPTY_DOCUMENT: RichTextValue = {
	schema: 'colophon/rich-text',
	version: 1,
	doc: { type: 'doc', content: [{ type: 'paragraph' }] }
};

/** One string per comparable state: fields in definition order, rich text in its canonical
 *  serialization. Two contents are the same edit exactly when these are equal. */
export function canonical(definition: Definition, content: Content): string {
	return JSON.stringify(
		definition.fields
			.filter((field) => content[field.id] !== undefined)
			.map((field) => [field.id, field.type === 'rich_text' ? serialize(content[field.id] as RichTextValue) : content[field.id]])
	);
}

export function same(definition: Definition, a: Content, b: Content): boolean {
	return canonical(definition, a) === canonical(definition, b);
}

export function titleOf(definition: Definition, content: Content): string {
	const title = content[definition.title_field];
	return typeof title === 'string' ? title : '';
}

/** What a new entry starts with: an empty document in required rich-text fields, false in
 *  required checkboxes, the first option of required choices; everything else is asked for. */
export function starter(definition: Definition): Content {
	return starterFields(definition.fields);
}

function starterFields(fields: FieldDefinition[]): Content {
	const content: Content = {};
	for (const field of fields) {
		if (field.default !== undefined) {
			content[field.id] = field.default;
			continue;
		}
		if (field.type === 'object') {
			// The editor always gets a subform to bind; an empty one stores as nothing.
			content[field.id] = starterFields(field.fields);
			continue;
		}
		if (field.type === 'list') {
			// The editor always gets an array to bind; an empty one stores as nothing.
			content[field.id] = [];
			continue;
		}
		if (field.type === 'blocks') {
			// A required minimum starts filled with sections of the first type.
			content[field.id] = Array.from({ length: field.min_blocks }, () => blockStarter(field.blocks[0]!));
			continue;
		}
		if (!field.required) continue;
		if (field.type === 'rich_text') content[field.id] = EMPTY_DOCUMENT;
		if (field.type === 'boolean') content[field.id] = false;
		if (field.type === 'enum') content[field.id] = field.values[0];
	}
	return content;
}

/** A form's value for a field: text inputs and selects work on strings, checkboxes on
 *  booleans, and Svelte binds number inputs as numbers (or null when empty). Rich text has its
 *  own editor. */
export type FormValue = string | boolean | number | null | SubForm | ListForm;
/** An object's subform: named fields of form values (interfaces recurse where aliases can't). */
export interface SubForm {
	[key: string]: FormValue;
}
/** A list's form value: items of its kind. */
export interface ListForm extends Array<FormValue> {}

/** A new section of a `blocks` field: `{"_block": "<type>", <starter fields>}`. */
export function blockStarter(block: { id: string; fields: FieldDefinition[] }): SubForm {
	return { _block: block.id, ...starterFields(block.fields) };
}

/** `YYYY-MM-DDTHH:MM:SSZ` to what a `datetime-local` input shows, in the browser's time zone. */
export function toLocal(utc: string): string {
	const date = new Date(utc);
	if (Number.isNaN(date.getTime())) return '';
	const pad = (n: number) => String(n).padStart(2, '0');
	return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}:${pad(date.getSeconds())}`;
}

/** A `datetime-local` value (local time) to `YYYY-MM-DDTHH:MM:SSZ`. */
export function toUtc(local: string): string {
	const date = new Date(local);
	return Number.isNaN(date.getTime()) ? local : `${date.toISOString().slice(0, 19)}Z`;
}

/** The form value showing a stored value. */
export function formValue(field: FieldDefinition, value: unknown): FormValue {
	switch (field.type) {
		case 'boolean':
			return value === true;
		case 'number':
			return typeof value === 'number' ? String(value) : '';
		case 'datetime':
			return typeof value === 'string' ? toLocal(value) : '';
		case 'json':
			return value === undefined || value === null ? '' : JSON.stringify(value, null, 2);
		case 'object':
			return Object.fromEntries(
				field.fields.map((sub) => [
					sub.id,
					formValue(sub, (value as Record<string, unknown> | undefined)?.[sub.id])
				])
			);
		case 'list':
			return Array.isArray(value) ? value.map((item) => formValue(itemField(field), item)) : [];
		case 'blocks':
			return Array.isArray(value)
				? value.map((item) => {
						const stored = (typeof item === 'object' && item !== null ? item : {}) as Record<string, unknown>;
						const block = field.blocks.find((block) => block.id === stored._block) ?? field.blocks[0]!;
						return {
							_block: String(stored._block ?? block.id),
							...Object.fromEntries(block.fields.map((sub) => [sub.id, formValue(sub, stored[sub.id])]))
						} as SubForm;
					})
				: [];
		default:
			return typeof value === 'string' ? value : '';
	}
}

/** A list item's shape as a field: the kind with its standard options (single-line text at
 *  the schema's maximum, a number of any size). Lets `formValue`/`storedValue` convert items
 *  with the same rules as fields. */
function itemField(field: Extract<FieldDefinition, { type: 'list' }>): FieldDefinition {
	const base = { id: '', label: '', required: true };
	switch (field.of) {
		case 'text':
			return { ...base, type: 'text', max_chars: 100000, multiline: false };
		case 'number':
			return { ...base, type: 'number', integer: false };
		default:
			return { ...base, type: field.of } as FieldDefinition;
	}
}

/** The stored value for a form value, or `undefined` to leave an optional field out. A
 *  required field left empty is still sent, so the server says which field and why. */
export function storedValue(field: FieldDefinition, form: FormValue | undefined): unknown {
	if (field.type === 'list') {
		const list = (Array.isArray(form) ? form : [])
			.map((item) => storedValue(itemField(field), item))
			.filter((item) => item !== undefined);
		if (list.length === 0) return field.required ? [] : undefined;
		return list;
	}
	if (field.type === 'blocks') {
		const list = (Array.isArray(form) ? form : [])
			.map((item) => {
				const stored = item as SubForm;
				const block = field.blocks.find((block) => block.id === stored._block);
				if (block === undefined) return undefined;
				const pairs = block.fields
					.map((sub) => [sub.id, storedValue(sub, stored[sub.id])] as const)
					.filter(([, value]) => value !== undefined);
				return { _block: stored._block, ...Object.fromEntries(pairs) };
			})
			.filter((item) => item !== undefined);
		if (list.length === 0) return field.required ? [] : undefined;
		return list;
	}
	if (field.type === 'object') {
		const subform = (typeof form === 'object' && form !== null ? form : {}) as Record<string, FormValue>;
		const pairs = field.fields
			.map((sub) => [sub.id, storedValue(sub, subform[sub.id])] as const)
			.filter(([, value]) => value !== undefined);
		// An empty optional object is left out; an empty required one is sent so the server
		// names every field it still needs.
		if (pairs.length === 0) return field.required ? {} : undefined;
		return Object.fromEntries(pairs);
	}
	if (field.type === 'boolean') return form === true;
	if (typeof form === 'number') return Number.isFinite(form) ? form : undefined;
	const text = typeof form === 'string' ? form : '';
	if (text === '') return field.required ? (field.type === 'text' ? '' : undefined) : undefined;
	switch (field.type) {
		case 'number':
			return Number(text);
		case 'datetime':
			return toUtc(text);
		case 'json':
			try {
				return JSON.parse(text);
			} catch {
				return text;
			}
		default:
			return text;
	}
}
