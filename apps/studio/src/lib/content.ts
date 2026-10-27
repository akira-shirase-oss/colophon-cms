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
	const content: Content = {};
	for (const field of definition.fields) {
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
export type FormValue = string | boolean | number | null;

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
		default:
			return typeof value === 'string' ? value : '';
	}
}

/** The stored value for a form value, or `undefined` to leave an optional field out. A
 *  required field left empty is still sent, so the server says which field and why. */
export function storedValue(field: FieldDefinition, form: FormValue | undefined): unknown {
	if (field.type === 'boolean') return form === true;
	if (typeof form === 'number') return Number.isFinite(form) ? form : undefined;
	const text = typeof form === 'string' ? form : '';
	if (text === '') return field.required ? (field.type === 'text' ? '' : undefined) : undefined;
	switch (field.type) {
		case 'number':
			return Number(text);
		case 'datetime':
			return toUtc(text);
		default:
			return text;
	}
}

/** The most characters in a slug, as the server allows (D-054). */
export const MAX_SLUG_CHARS = 120;

/** The server's slug rule: lowercase ASCII letters and digits, in groups joined by single hyphens. */
export const SLUG_PATTERN = /^[a-z0-9]+(-[a-z0-9]+)*$/;

/** A slug suggested from text, such as a title: accents dropped, everything else that isn't a
 *  letter or digit becomes one hyphen. The server never does this; it refuses what doesn't match
 *  [`SLUG_PATTERN`], so what is stored is what the editor saw. `''` when nothing is left. */
export function slugify(text: string): string {
	const plain = text
		.normalize('NFKD')
		.replace(/\p{M}+/gu, '')
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');
	return plain.slice(0, MAX_SLUG_CHARS).replace(/-+$/, '');
}
