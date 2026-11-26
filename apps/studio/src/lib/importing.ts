// Bringing content in from somewhere else (the Import content page): the pure parts. Reading a
// file into items the way the server's import reads a body, reading a Markdown file's front
// matter, matching its names to a content type's fields, cutting a long list into batches, and
// saying a check's or a run's result in plain words.
//
// The server is the authority (D-086, D-114): `POST {P}/collections/{collection}/import` maps,
// validates and writes every item, and `?dry_run=true` answers what each would do. Nothing here
// decides whether an entry is valid. What happens here is only what the server leaves to its
// caller: a file is split into items so a long one can go in several requests, a Markdown file
// becomes the document its front matter and body describe, and values are put in the form the
// server's validation asks for (a date field's `YYYY-MM-DD`, no empty strings, no carriage
// returns), each rule taken from `colophon_domain::schema`.

import { parse as parseYaml } from 'yaml';
import type { Content, Definition, FieldDefinition, ImportOutcome } from './api.ts';
import { slugify } from './content.ts';
import { plainOr } from './errors.ts';
import { count } from './words.ts';

/** Where the content is now. */
export type SourceId = 'strapi' | 'sanity' | 'markdown' | 'colophon' | 'json';

/** The server takes at most this many items in one import (`IMPORT_MAX_ITEMS`). */
export const SERVER_MAX_ITEMS = 512;
/** How many the Studio sends at once: well under the server's 512, so one batch is a short
 *  transaction that answers inside the request deadline on a small server too. */
export const BATCH_ITEMS = 100;
/** The most bytes in one batch; the server refuses a body over 16 MiB. */
export const BATCH_BYTES = 4 * 1024 * 1024;
/** The largest JSON or line-per-entry file read here. */
export const MAX_FILE_BYTES = 32 * 1024 * 1024;
/** The largest Markdown file read here: one entry, so the same as one batch. */
export const MAX_MARKDOWN_BYTES = BATCH_BYTES;
/** The most entries one import run carries (an export holds at most as many). */
export const MAX_ITEMS = 10_000;

const isRecord = (value: unknown): value is Record<string, unknown> => typeof value === 'object' && value !== null && !Array.isArray(value);

// ---- Front matter

export type FrontMatter =
	| {
			ok: true;
			/** The names and values between the `---` lines; empty when the file has none. */
			data: Record<string, unknown>;
			/** Everything after the front matter, with line endings as `\n` and no blank ends. */
			body: string;
			/** Whether the file had front matter at all. */
			found: boolean;
	  }
	| { ok: false; problem: string; technical: string };

/** A Markdown file as its front matter and its body. The front matter is YAML between two `---`
 *  lines at the very top (what Astro, Hugo, Jekyll, Eleventy and Next.js blogs write by default);
 *  `+++` (TOML) is said to be unread rather than guessed at. */
export function parseFrontMatter(raw: string): FrontMatter {
	const text = raw.replace(/^﻿/, '').replace(/\r\n?/g, '\n');
	const tidy = (body: string) => body.replace(/^\n+/, '').replace(/\s+$/, '');
	if (/^\+\+\+[ \t]*(\n|$)/.test(text)) {
		return { ok: false, problem: 'Its front matter is between +++ lines (TOML). Only front matter between --- lines (YAML) is read.', technical: 'TOML front matter' };
	}
	const open = /^---[ \t]*\n/.exec(text);
	if (open === null) return { ok: true, data: {}, body: tidy(text), found: false };
	const rest = text.slice(open[0].length);
	const close = /(^|\n)(---|\.\.\.)[ \t]*(\n|$)/.exec(rest);
	if (close === null) {
		return { ok: false, problem: 'Its front matter starts with a --- line and has no --- line to end it.', technical: 'no closing fence' };
	}
	let data: unknown;
	try {
		data = parseYaml(rest.slice(0, close.index));
	} catch (failure) {
		const line = (failure as { linePos?: { line: number }[] }).linePos?.[0]?.line;
		return {
			ok: false,
			// The front matter starts on the file's second line.
			problem: line === undefined ? 'Its front matter can’t be read.' : `Its front matter can’t be read: look at line ${line + 1} of the file.`,
			technical: failure instanceof Error ? failure.message : String(failure)
		};
	}
	if (data === null || data === undefined) data = {};
	if (!isRecord(data)) {
		return { ok: false, problem: 'Its front matter isn’t a list of names and values, like “title: My post”.', technical: 'the front matter is not a mapping' };
	}
	return { ok: true, data, body: tidy(rest.slice(close.index + close[0].length)), found: true };
}

// ---- Files of items

export type ItemsRead = { ok: true; items: unknown[] } | { ok: false; problem: string };

/** A file's text as import items, read the way the server reads a body (`import_items`): one JSON
 *  list, a response with a `data` list beside `meta`, or one JSON object per line. A single
 *  object spread over several lines is one item here; the server would take it for lines. */
export function readItems(raw: string): ItemsRead {
	const text = raw.replace(/^﻿/, '').trim();
	if (text === '') return { ok: false, problem: 'This file is empty.' };
	const parse = (source: string): { value: unknown } | null => {
		try {
			return { value: JSON.parse(source) as unknown };
		} catch {
			return null;
		}
	};
	if (text.startsWith('[')) {
		const whole = parse(text);
		return whole !== null && Array.isArray(whole.value)
			? { ok: true, items: whole.value }
			: { ok: false, problem: 'This file starts like a JSON list and can’t be read as one. Check that it was saved whole.' };
	}
	if (text.startsWith('{')) {
		const whole = parse(text);
		if (whole !== null && isRecord(whole.value)) {
			const { data, meta } = whole.value;
			if (meta !== undefined && meta !== null && Array.isArray(data)) return { ok: true, items: data };
			// One entry's answer: `data` is the entry, and nothing sits beside it but `meta`.
			if (meta !== undefined && meta !== null && isRecord(data) && Object.keys(whole.value).every((key) => key === 'data' || key === 'meta')) {
				return { ok: true, items: [data] };
			}
			return { ok: true, items: [whole.value] };
		}
	}
	const items: unknown[] = [];
	for (const [index, line] of text.split('\n').entries()) {
		if (line.trim() === '') continue;
		const item = parse(line.trim());
		if (item === null) {
			return { ok: false, problem: `Line ${index + 1} of this file isn’t JSON. Each line should be one entry, or the whole file one JSON list.` };
		}
		items.push(item.value);
	}
	return { ok: true, items };
}

/** Whether these first bytes are an archive (gzip, zip) rather than text: an export that still
 *  needs unpacking. */
export function isArchive(head: Uint8Array): boolean {
	return (head[0] === 0x1f && head[1] === 0x8b) || (head[0] === 0x50 && head[1] === 0x4b);
}

/** A Strapi export's line: `{"type": "api::article.article", "data": {…}}` (D-086). */
const markedType = (item: Record<string, unknown>): string | null =>
	typeof item['type'] === 'string' && item['type'].includes('::') && 'data' in item ? item['type'] : null;

/** What an item looks like it came from, by the shapes the server's mapping tells apart
 *  (`colophon_domain::transfer::map`): a Sanity document has `_id` or `_type`; a Strapi one is a
 *  marked export line, an `{id, attributes}` entity or a flat document with a `documentId`. A
 *  plain document is anything else. */
export function shapeOf(item: unknown): 'sanity' | 'strapi' | 'plain' {
	if (!isRecord(item)) return 'plain';
	if (markedType(item) !== null) return 'strapi';
	if ('_id' in item || '_type' in item) return 'sanity';
	if (isRecord(item['attributes']) && ('id' in item || 'documentId' in item)) return 'strapi';
	if ('documentId' in item || 'document_id' in item) return 'strapi';
	return 'plain';
}

/** Why `items` aren't what `source` says, or `null` when they could be. Only a file where no item
 *  has the source's shape is turned away; a mixed one is the check's to judge item by item. */
export function wrongSource(items: unknown[], source: SourceId): string | null {
	if (source === 'markdown' || items.length === 0) return null;
	const shapes = new Set(items.map(shapeOf));
	if (source === 'sanity' && !shapes.has('sanity')) {
		return shapes.has('strapi')
			? 'This file looks like it came from Strapi, not Sanity. Choose Strapi above.'
			: 'This doesn’t look like a Sanity export: none of its documents has a “_type”. If it is a plain JSON file, choose JSON file above.';
	}
	if (source === 'strapi' && !shapes.has('strapi')) {
		return shapes.has('sanity')
			? 'This file looks like it came from Sanity, not Strapi. Choose Sanity above.'
			: 'This doesn’t look like it came from Strapi: none of its entries has an “id” with “attributes”, or a “documentId”. If it is a plain JSON file, choose JSON file above.';
	}
	if ((source === 'colophon' || source === 'json') && !shapes.has('plain')) {
		return shapes.has('sanity') ? 'This file looks like a Sanity export. Choose Sanity above.' : 'This file looks like it came from Strapi. Choose Strapi above.';
	}
	return null;
}

/** The kind a source gives an item: a Sanity document's `_type`, a Strapi export line's `type`.
 *  `null` when it names none (a Strapi REST answer holds one kind and doesn't say which). */
export function kindOf(item: unknown, source: SourceId): string | null {
	if (!isRecord(item)) return null;
	if (source === 'sanity') return typeof item['_type'] === 'string' ? item['_type'] : null;
	if (source === 'strapi') return markedType(item);
	return null;
}

export interface Kind {
	kind: string;
	count: number;
}

/** The kinds in a file, most first: what a mixed export holds, so one can be chosen. */
export function kindsOf(items: unknown[], source: SourceId): Kind[] {
	const counts = new Map<string, number>();
	for (const item of items) {
		const kind = kindOf(item, source);
		if (kind !== null) counts.set(kind, (counts.get(kind) ?? 0) + 1);
	}
	return [...counts].map(([kind, count]) => ({ kind, count })).sort((a, b) => b.count - a.count || a.kind.localeCompare(b.kind));
}

/** The kind to start with for a content type: the one named like it (`posts` for `posts`,
 *  `api::article.article` for `articles`), else the most common that isn't the source's own
 *  bookkeeping (Sanity's `sanity.` and `system.` documents, Strapi's `plugin::` and `admin::`). */
export function suggestedKind(kinds: Kind[], collection: string): string | null {
	const plain = (text: string) => normalKey(text).replace(/s$/, '');
	const wanted = plain(collection);
	const named = kinds.find(({ kind }) => plain(kind.split(/[.:]+/).at(-1) ?? kind) === wanted);
	if (named !== undefined) return named.kind;
	const own = kinds.find(({ kind }) => !/^(sanity\.|system\.|plugin::|admin::|strapi::)/.test(kind));
	return (own ?? kinds[0])?.kind ?? null;
}

/** A name for an item in a list of problems: its title where one can be found, as the source
 *  nests it. `''` when there is none to show. */
export function titleOf(item: unknown, titleField: string): string {
	let at: unknown = item;
	for (let depth = 0; depth < 4 && isRecord(at); depth += 1) {
		const title = at[titleField];
		if (typeof title === 'string' && title.trim() !== '') return title.trim();
		at = isRecord(at['attributes']) ? at['attributes'] : at['data'];
	}
	return '';
}

// ---- Names to fields

/** A name with its case, spaces, hyphens and underscores taken out: `pubDate`, `pub_date` and
 *  `Pub Date` are one name. */
export function normalKey(key: string): string {
	return key.toLowerCase().replace(/[^a-z0-9]/g, '');
}

/** Which field each name goes into; `null` leaves it out. */
export type Mapping = Record<string, string | null>;

/** Every name in `records`, in the order first met. */
export function namesIn(records: Record<string, unknown>[]): string[] {
	const names = new Set<string>();
	for (const record of records) for (const key of Object.keys(record)) names.add(key);
	return [...names];
}

/** Matches names to `fields`: first by the field's exact name in code, then ignoring case, spaces,
 *  hyphens and underscores (`pubDate` → `pub_date`). A field takes one name, the first that fits;
 *  a name that fits nothing is left out (`null`). `reserved` fields are never offered (the one the
 *  Markdown body fills). */
export function autoMap(names: string[], fields: FieldDefinition[], reserved: string[] = []): Mapping {
	const open = new Set(fields.map((field) => field.id).filter((id) => !reserved.includes(id)));
	const mapping: Mapping = Object.fromEntries(names.map((name) => [name, null]));
	for (const name of names) {
		if (open.has(name)) {
			mapping[name] = name;
			open.delete(name);
		}
	}
	for (const name of names) {
		if (mapping[name] !== null) continue;
		const found = [...open].find((id) => normalKey(id) === normalKey(name) && normalKey(name) !== '');
		if (found !== undefined) {
			mapping[name] = found;
			open.delete(found);
		}
	}
	return mapping;
}

/** A date field's value: `YYYY-MM-DD`. A full timestamp keeps its date as written. */
export function dateOnly(value: unknown): unknown {
	if (typeof value !== 'string') return value;
	const match = /^(\d{4}-\d{2}-\d{2})(?:[T ].*)?$/.exec(value.trim());
	return match?.[1] ?? value;
}

/** A date-and-time field's value: the server takes UTC written as `2026-10-25T14:30:00Z` and
 *  nothing else. A date alone is its midnight in UTC; a time with an offset is moved to UTC; one
 *  with no zone is read as UTC. What isn't a timestamp is left for the server to refuse. */
export function utcTimestamp(value: unknown): unknown {
	if (typeof value !== 'string') return value;
	const match = /^(\d{4}-\d{2}-\d{2})(?:[T ](\d{2}:\d{2})(:\d{2})?(?:\.\d+)?\s*(Z|[+-]\d{2}:?\d{2})?)?$/i.exec(value.trim());
	if (match === null) return value;
	const [, day, minutes = '00:00', seconds = ':00', zone = 'Z'] = match;
	const offset = zone.toUpperCase() === 'Z' ? 'Z' : zone.includes(':') ? zone : `${zone.slice(0, 3)}:${zone.slice(3)}`;
	// A day its month doesn't have (the 31st of February) is not moved to the next month.
	const [year = 0, month = 0, date = 0] = (day ?? '').split('-').map(Number);
	if (new Date(Date.UTC(year, month - 1, date)).getUTCMonth() !== month - 1) return value;
	const moment = new Date(`${day}T${minutes}${seconds}${offset}`);
	return Number.isNaN(moment.getTime()) ? value : `${moment.toISOString().slice(0, 19)}Z`;
}

/** `value` in the form `field` stores, or `undefined` to leave the field out. Only what the
 *  server's validation would refuse for its form alone is changed: nothing empty (it says "leave
 *  it out instead"), no spaces at the ends of one-line text, `\n` line endings, a number or yes/no
 *  as the text it was written as, and dates as above. Whether the value is right is the server's. */
export function coerce(value: unknown, field: { type: string; multiline?: boolean; of?: { type: string; multiline?: boolean } }): unknown {
	if (value === null || value === undefined) return undefined;
	const text = (trimmed: boolean) => {
		if (typeof value === 'number' || typeof value === 'boolean') return String(value);
		if (typeof value !== 'string') return value;
		const unix = value.replace(/\r\n?/g, '\n');
		const kept = trimmed ? unix.trim() : unix;
		return kept === '' ? undefined : kept;
	};
	switch (field.type) {
		case 'text':
			return text(field.multiline !== true);
		case 'markdown':
			return text(false);
		case 'slug':
		case 'enum':
		case 'email':
		case 'url':
		case 'color':
		case 'time':
			return text(true);
		case 'date':
			return typeof value === 'string' && value.trim() === '' ? undefined : dateOnly(value);
		case 'datetime':
			return typeof value === 'string' && value.trim() === '' ? undefined : utcTimestamp(value);
		case 'number':
			return typeof value === 'string' && value.trim() !== '' && Number.isFinite(Number(value)) ? Number(value) : value === '' ? undefined : value;
		case 'boolean':
			return value === 'true' ? true : value === 'false' ? false : value === '' ? undefined : value;
		case 'list': {
			const of = field.of;
			return Array.isArray(value) && of !== undefined ? value.map((item) => coerce(item, of)).filter((item) => item !== undefined) : value;
		}
		default:
			return value === '' ? undefined : value;
	}
}

/** `record` as a document for a content type: each name's value under the field `mapping` gives
 *  it, in the field's form; names mapped to nothing are left out. */
export function mapRecord(record: Record<string, unknown>, mapping: Mapping, fields: FieldDefinition[]): Content {
	const document: Content = {};
	for (const [name, value] of Object.entries(record)) {
		const field = fields.find((one) => one.id === mapping[name]);
		if (field === undefined) continue;
		const stored = coerce(value, field);
		if (stored !== undefined) document[field.id] = stored;
	}
	return document;
}

/** A content type's Markdown fields: where a Markdown file's body can go. */
export function markdownFields(definition: Pick<Definition, 'fields'>): FieldDefinition[] {
	return definition.fields.filter((field) => field.type === 'markdown');
}

/** A content type's URL-name field, if it has one. */
export function slugFieldOf(definition: Pick<Definition, 'fields'>): FieldDefinition | undefined {
	return definition.fields.find((field) => field.type === 'slug');
}

/** A URL name from a file's name: `posts/Hello World.md` → `hello-world`. A page kept as
 *  `hello-world/index.md` takes its folder's name. `''` when nothing is left. */
export function slugFromPath(path: string): string {
	const parts = path.split(/[/\\]/).filter((part) => part !== '');
	const stem = (parts.at(-1) ?? '').replace(/\.[^.]+$/, '');
	const folder = parts.at(-2);
	return slugify(/^_?index$/i.test(stem) && folder !== undefined ? folder : stem);
}

/** One Markdown file as the entry it describes: its front matter by `mapping`, its body in
 *  `bodyField`, and its URL name from the file's name when the front matter gave none. */
export function markdownEntry(
	path: string,
	front: { data: Record<string, unknown>; body: string },
	mapping: Mapping,
	definition: Pick<Definition, 'fields'>,
	bodyField: string
): Content {
	const fields = definition.fields.filter((field) => field.id !== bodyField);
	const document = mapRecord(front.data, mapping, fields);
	const body = coerce(front.body, { type: 'markdown' });
	if (body !== undefined) document[bodyField] = body;
	const slug = slugFieldOf(definition);
	if (slug !== undefined && document[slug.id] === undefined) {
		const made = slugFromPath(path);
		if (made !== '') document[slug.id] = made;
	}
	return document;
}

// ---- Batches

/** How many bytes `value` is as JSON. */
export function jsonBytes(value: unknown): number {
	return new TextEncoder().encode(JSON.stringify(value) ?? 'null').length;
}

/** `items` in order, cut into batches of at most `maxItems` items and `maxBytes` bytes of JSON.
 *  An item over `maxBytes` by itself still goes, alone: the server says whether it is too large. */
export function batches<T>(items: T[], maxItems = BATCH_ITEMS, maxBytes = BATCH_BYTES, bytesOf: (item: T) => number = jsonBytes): T[][] {
	const limit = Math.max(1, Math.min(maxItems, SERVER_MAX_ITEMS));
	const out: T[][] = [];
	let batch: T[] = [];
	let bytes = 0;
	for (const item of items) {
		const size = bytesOf(item) + 1;
		if (batch.length > 0 && (batch.length >= limit || bytes + size > maxBytes)) {
			out.push(batch);
			batch = [];
			bytes = 0;
		}
		batch.push(item);
		bytes += size;
	}
	if (batch.length > 0) out.push(batch);
	return out;
}

// ---- A check's result

/** One entry the check found it can't import. */
export interface PlanProblem {
	/** Its place among the entries checked, from 0. */
	index: number;
	/** The label of the field the refusal points at, when it points at one of this type's. */
	field: string | null;
	/** Why, in plain words. */
	text: string;
	/** The server's code, pointer and message, for the Technical details. */
	technical: string;
}

export interface PlanSummary {
	create: number;
	update: number;
	skipped: number;
	invalid: number;
	problems: PlanProblem[];
	/** Names from the source that were set aside, not stored (`unmapped`), across every entry. */
	setAside: string[];
}

/** A JSON Pointer's parts, unescaped. */
function segments(pointer: string): string[] {
	return pointer
		.split('/')
		.slice(1)
		.map((part) => part.replaceAll('~1', '/').replaceAll('~0', '~'));
}

/** Where a refusal points inside its item: the pointer without the item's own place (`/3/title`
 *  for item `/3` → `/title`). Refusals about the whole item carry no pointer. */
export function pointerInItem(pointer: string, item: string): string {
	return item !== '' && (pointer === item || pointer.startsWith(`${item}/`)) ? pointer.slice(item.length) : pointer;
}

/** A refusal's place in a JSON list body: `/12/title` → item 12, `/title`. */
export function placeOf(pointer: string): { index: number; rest: string } | null {
	const match = /^\/(\d+)(\/.*)?$/.exec(pointer);
	return match === null ? null : { index: Number(match[1]), rest: match[2] ?? '' };
}

/** Why an entry can't be imported, in plain words, with the field it is about. `pointer` is
 *  inside the item. The server's message is used when it has no internal words in it; a few codes
 *  whose messages speak of the API get their own. */
export function refusalWords(
	refusal: { code: string; pointer: string; message: string },
	definition: Pick<Definition, 'fields'>
): { field: string | null; text: string } {
	const path = segments(refusal.pointer);
	const field = definition.fields.find((one) => one.id === path[0])?.label ?? null;
	const fallback = 'Colophon can’t take this entry as it is.';
	switch (refusal.code) {
		case 'unknown_field':
			return { field: null, text: `It has “${path.at(-1) ?? ''}”, and this content type has no field with that name.` };
		case 'type_mismatch':
			return { field: null, text: 'It is a different kind of document from the one chosen.' };
		case 'slug_taken':
			return { field, text: 'Another entry already uses this URL name.' };
		case 'value_taken':
			return { field, text: 'Another entry already has this value, and no two entries may share it.' };
		case 'missing_permission':
			return { field: null, text: 'Your role can’t create or change this entry.' };
		case 'budget_exceeded':
			return { field: null, text: 'It is over the number of entries that may be written in a day.' };
		case 'single_exists':
			return { field: null, text: 'This content type holds one entry, and it already has it.' };
		case 'not_found':
		case 'stale_revision':
			return { field: null, text: 'It names an entry that is gone or that changed just now.' };
		case 'not_an_object':
			return path.length === 0 ? { field: null, text: 'It isn’t a set of named fields.' } : { field, text: plainOr(refusal.message, fallback) };
		default:
			return { field, text: plainOr(refusal.message, fallback) };
	}
}

/** A dry run's outcomes as what will happen: how many will be created, updated, skipped, and
 *  which can't be imported and why. `outcomes` are in the order the entries were checked. */
export function summarize(outcomes: ImportOutcome[], definition: Pick<Definition, 'fields'>): PlanSummary {
	const summary: PlanSummary = { create: 0, update: 0, skipped: 0, invalid: 0, problems: [], setAside: [] };
	const setAside = new Set<string>();
	for (const [index, outcome] of outcomes.entries()) {
		for (const name of outcome.unmapped ?? []) setAside.add(name);
		switch (outcome.status) {
			case 'would_create':
			case 'created':
				summary.create += 1;
				break;
			case 'would_update':
			case 'updated':
				summary.update += 1;
				break;
			case 'skipped':
				summary.skipped += 1;
				break;
			case 'invalid': {
				summary.invalid += 1;
				const refusal = outcome.refusal ?? { code: 'invalid', pointer: '', message: '' };
				const inItem = { ...refusal, pointer: pointerInItem(refusal.pointer, outcome.item) };
				summary.problems.push({
					index,
					...refusalWords(inItem, definition),
					technical: [refusal.code, refusal.pointer, refusal.message].filter((part) => part !== '').join(' · ')
				});
				break;
			}
		}
	}
	summary.setAside = [...setAside].sort();
	return summary;
}

/** Which of the checked entries the import sends: the ones the check would create or update. */
export function importable(outcomes: ImportOutcome[]): number[] {
	return outcomes.flatMap((outcome, index) => (outcome.status === 'would_create' || outcome.status === 'would_update' ? [index] : []));
}

// ---- A run's result

/** Where an import run stands, or ended. */
export interface Run {
	/** How many entries each batch holds, in order. */
	sizes: number[];
	/** Batches the server answered as imported, from the first. */
	done: number;
	/** The batch the run stopped at, if it stopped: `refused` means the server turned it away and
	 *  wrote none of it; `unanswered` means no answer came, so whether it landed isn't known. */
	stopped: null | { batch: number; how: 'refused' | 'unanswered' };
}

/** A run's result, said truthfully: every entry imported, or how many landed, which batch
 *  stopped it, and what was never sent. Never "imported" for a run that stopped. */
export function runWords(run: Run): { ok: boolean; title: string; text: string } {
	const total = run.sizes.reduce((sum, size) => sum + size, 0);
	const landed = run.sizes.slice(0, run.done).reduce((sum, size) => sum + size, 0);
	const entries = (n: number) => count(n, 'entry', 'entries');
	if (run.stopped === null) {
		return { ok: true, title: `Imported ${entries(landed)}`, text: 'They are drafts. Nothing is published until you publish it.' };
	}
	const { batch, how } = run.stopped;
	const were = (n: number) => (n === 1 ? 'was' : 'were');
	if (run.sizes.length === 1) {
		return how === 'refused'
			? { ok: false, title: 'Nothing was imported', text: `The import was refused, so none of the ${entries(total)} ${were(total)} imported.` }
			: {
					ok: false,
					title: 'The import didn’t finish',
					text: `Colophon didn’t answer, so it isn’t known whether the ${entries(total)} ${were(total)} imported. Try again: the same entries are never added twice.`
				};
	}
	const size = run.sizes[batch] ?? 0;
	const after = total - landed - size;
	const which = `Batch ${batch + 1} of ${run.sizes.length} (${size === 1 ? `entry ${(landed + 1).toLocaleString()}` : `entries ${(landed + 1).toLocaleString()} to ${(landed + size).toLocaleString()}`})`;
	const before = landed === 0 ? 'Nothing before it was imported' : `The ${entries(landed)} before it ${landed === 1 ? 'is' : 'are'} in, as ${landed === 1 ? 'a draft' : 'drafts'}`;
	const rest = after === 0 ? '' : ` The ${entries(after)} after it ${were(after)} not sent.`;
	if (how === 'refused') {
		return {
			ok: false,
			title: landed === 0 ? 'Nothing was imported' : `Imported ${landed.toLocaleString()} of ${entries(total)}`,
			text: `${which} was refused, and none of that batch was imported. ${before}.${rest}`
		};
	}
	return {
		ok: false,
		title: landed === 0 ? 'The import didn’t finish' : `Imported ${landed.toLocaleString()} of ${entries(total)} so far`,
		text: `${which} got no answer, so it isn’t known whether ${size === 1 ? 'it was' : 'those were'} imported. ${before}.${rest} Try again: the same batch is never added twice.`
	};
}
