// Content and its content type: what the editor needs to build, compare and show an entry
// generically, whatever fields its type has (D-040).

import { type RichTextValue, serialize } from '@colophon/editor';
import type { Content, Definition, FieldDefinition } from './api.ts';
import { canonicalJson, prettyJson, readJson } from './json.ts';

export const EMPTY_DOCUMENT: RichTextValue = {
	schema: 'colophon/rich-text',
	version: 1,
	doc: { type: 'doc', content: [{ type: 'paragraph' }] }
};

/** One string per comparable state: fields in definition order, rich text in its canonical
 *  serialization, JSON in the server's canonical form (whatever key order it arrived in). Two
 *  contents are the same edit exactly when these are equal. */
export function canonical(definition: Definition, content: Content): string {
	return JSON.stringify(
		definition.fields
			.filter((field) => content[field.id] !== undefined)
			.map((field) => [field.id, comparable(field, content[field.id])])
	);
}

function comparable(field: FieldDefinition, value: unknown): unknown {
	if (field.type === 'rich_text') return serialize(value as RichTextValue);
	if (field.type === 'json') {
		try {
			return canonicalJson(value);
		} catch {
			return JSON.stringify(value);
		}
	}
	return value;
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
		case 'json':
			return value === undefined ? '' : prettyJson(value);
		default:
			return typeof value === 'string' ? value : '';
	}
}

/** A web address as the server stores it (`new URL(text).href`), or the text when it isn't one. */
export function webAddress(text: string): string {
	try {
		return new URL(text).href;
	} catch {
		return text;
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
		case 'url':
			return webAddress(text.trim());
		case 'color':
			return text.trim().toLowerCase();
		case 'json':
			// Never sent while it doesn't read as JSON: the editor holds the save (fieldProblem).
			return readJson(text).value;
		default:
			return text;
	}
}

/** Characters as the server counts them: Unicode scalar values, not UTF-16 units. */
export const characters = (text: string) => [...text].length;

const HOST_LABEL = /^[A-Za-z0-9](?:[A-Za-z0-9-]{0,61}[A-Za-z0-9])?$/;
const LOCAL = /^[A-Za-z0-9!#$%&'*+\-/=?^_`{|}~.]+$/;

/** The server's shape check for an email address (schema.rs `is_email`): never delivery. */
export function isEmail(text: string): boolean {
	const at = text.indexOf('@');
	if (at < 0 || text.indexOf('@', at + 1) >= 0 || text.length > 254) return false;
	const local = text.slice(0, at);
	const domain = text.slice(at + 1);
	return (
		local.length >= 1 &&
		local.length <= 64 &&
		!local.startsWith('.') &&
		!local.endsWith('.') &&
		!local.includes('..') &&
		LOCAL.test(local) &&
		domain.includes('.') &&
		domain.length <= 253 &&
		domain.split('.').every((label) => HOST_LABEL.test(label))
	);
}

/** The server's check for a web address: http or https, a host, no user name or password. */
export function isWebAddress(text: string): boolean {
	if (text.length > 2048 || /\s/.test(text)) return false;
	try {
		const url = new URL(text);
		return (url.protocol === 'http:' || url.protocol === 'https:') && url.hostname !== '' && url.username === '' && url.password === '' && url.href.length <= 2048;
	} catch {
		return false;
	}
}

/** `YYYY-MM-DD`, a real calendar date. */
export function isDate(text: string): boolean {
	const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(text);
	if (match === null) return false;
	const [year, month, day] = [Number(match[1]), Number(match[2]), Number(match[3])];
	const date = new Date(Date.UTC(year, month - 1, day));
	return date.getUTCFullYear() === year && date.getUTCMonth() === month - 1 && date.getUTCDate() === day;
}

/** `HH:MM` or `HH:MM:SS`, 24-hour. */
export function isTime(text: string): boolean {
	const match = /^(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(text);
	return match !== null && Number(match[1]) < 24 && Number(match[2]) < 60 && Number(match[3] ?? '0') < 60;
}

/**
 * What the server would refuse in a field's form value, in its words, or '' when it looks fine
 * (or is empty: a missing required value is the server's to say). The server still decides.
 */
export function fieldProblem(field: FieldDefinition, form: FormValue | undefined): string {
	const text = typeof form === 'string' ? form : '';
	if (text === '' || typeof form !== 'string') return '';
	switch (field.type) {
		case 'text':
			if (field.min_chars !== undefined && characters(text) < field.min_chars) return `${field.label} is at least ${field.min_chars} characters.`;
			if (characters(text) > field.max_chars) return `${field.label} is at most ${field.max_chars} characters.`;
			return '';
		case 'markdown':
			return characters(text) > field.max_chars ? `${field.label} is at most ${field.max_chars.toLocaleString()} characters.` : '';
		case 'email':
			return isEmail(text) ? '' : `${field.label} is an email address like name@example.com.`;
		case 'url':
			return isWebAddress(text.trim()) ? '' : `${field.label} is a web address starting with https:// or http://.`;
		case 'date':
			return isDate(text) ? '' : `${field.label} is a date like 2026-10-28.`;
		case 'time':
			return isTime(text) ? '' : `${field.label} is a time like 14:30 or 14:30:15.`;
		case 'color':
			return /^#[0-9a-f]{6}$/.test(text.trim().toLowerCase()) ? '' : `${field.label} is a colour like #1a2b3c.`;
		case 'json':
			return readJson(text).problem ?? '';
		default:
			return '';
	}
}

/** A JSON field whose text can't be stored holds the save: sending it would lose or change it. */
export function blockingProblem(fields: FieldDefinition[], form: Record<string, FormValue>): string {
	for (const field of fields) {
		if (field.type !== 'json') continue;
		const problem = fieldProblem(field, form[field.id]);
		if (problem !== '') return `${field.label}: ${problem}`;
	}
	return '';
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
