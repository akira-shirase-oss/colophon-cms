// An entry's history as people read it (lib/history.ts, D-184): saves one person made within five
// minutes of each other are one sitting, the live version always ends one, and each sitting says
// what it changed in words.

import { expect, test } from '@playwright/test';
import type { Definition, Revision } from '../src/lib/api.ts';
import { changeSummary, initials, sittings, SITTING_GAP_MS } from '../src/lib/history.ts';

const at = (minutes: number) => new Date(Date.UTC(2026, 10, 18, 9, 0) + minutes * 60_000).toISOString();
const revision = (id: string, author: string, minutes: number): Revision => ({
	id,
	base_revision_id: null,
	author_id: author,
	created_at: at(minutes),
	content_hash: `hash-${id}`,
	title: 'Tide tables'
});

const definition: Definition = {
	name: 'Articles',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'summary', label: 'Summary', type: 'text', required: false, max_chars: 300, multiline: true },
		{ id: 'body', label: 'Body', type: 'rich_text', required: false },
		{ id: 'featured', label: 'Featured', type: 'boolean', required: false }
	]
};

const doc = (...blocks: object[]) => ({ schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: blocks } });
const paragraph = (text: string) => ({ type: 'paragraph', content: [{ type: 'text', text }] });
const image = { type: 'image', attrs: { assetId: '01a1e5e6-0000-7000-8000-000000000000', alt: 'A boat' } };

test('saves by one person within five minutes are one sitting; a gap or another person starts the next', () => {
	// Newest first, as the API lists them.
	const list = [
		revision('r6', 'ana', 40),
		revision('r5', 'ana', 38),
		revision('r4', 'ben', 37),
		revision('r3', 'ana', 20),
		revision('r2', 'ana', 16),
		revision('r1', 'ana', 12)
	];
	const grouped = sittings(list);
	expect(grouped.map((sitting) => sitting.revisions.map((one) => one.id))).toEqual([['r6', 'r5'], ['r4'], ['r3', 'r2', 'r1']]);
	expect(grouped[0]?.last.id).toBe('r6');
	expect(grouped[0]?.first.id).toBe('r5');
	expect(grouped[2]?.author).toBe('ana');
	// Exactly five minutes apart still joins; a moment more doesn't.
	expect(SITTING_GAP_MS).toBe(300_000);
	expect(sittings([revision('b', 'ana', 5), revision('a', 'ana', 0)])).toHaveLength(1);
	expect(sittings([revision('b', 'ana', 5.01), revision('a', 'ana', 0)])).toHaveLength(2);
});

test('the live version always ends its sitting, so restoring that sitting restores it', () => {
	const list = [revision('r3', 'ana', 3), revision('r2', 'ana', 2), revision('r1', 'ana', 1)];
	const grouped = sittings(list, ['r2']);
	expect(grouped.map((sitting) => sitting.revisions.map((one) => one.id))).toEqual([['r3'], ['r2', 'r1']]);
	expect(grouped[1]?.last.id).toBe('r2');
	// Nothing live: one sitting.
	expect(sittings(list, [null])).toHaveLength(1);
});

test('a sitting says what it changed: fields by name, rich text by what was added or taken out', () => {
	const before = { title: 'Tide tables', body: doc(paragraph('High water at six.')) };
	expect(changeSummary(definition, null, before)).toBe('First version');
	expect(changeSummary(definition, before, before)).toBe('No change to the content');
	expect(changeSummary(definition, before, { ...before, title: 'Tide tables for March' })).toBe('Title edited');
	expect(changeSummary(definition, before, { ...before, body: doc(paragraph('High water at six.'), paragraph('Low at noon.'), paragraph('High again.'), image) })).toBe(
		'Body: +2 paragraphs, +1 image'
	);
	expect(changeSummary(definition, before, { ...before, body: doc() })).toBe('Body: −1 paragraph');
	expect(changeSummary(definition, before, { ...before, body: doc(paragraph('High water at seven.')) })).toBe('Body: edited');
	expect(changeSummary(definition, before, { ...before, summary: 'Spring tides' })).toBe('Summary added');
	expect(changeSummary(definition, { ...before, summary: 'Spring tides' }, before)).toBe('Summary cleared');
	// At most three, then how many more.
	expect(
		changeSummary(definition, before, { title: 'New', summary: 'Added', body: doc(), featured: true })
	).toBe('Title edited · Summary added · Body: −1 paragraph · and 1 more');
});

test('initials come from up to two words of a name', () => {
	expect(initials('Dev Owner')).toBe('DO');
	expect(initials('  marie  kerjean le goff ')).toBe('MK');
	expect(initials('Ana')).toBe('A');
	expect(initials('')).toBe('');
});
