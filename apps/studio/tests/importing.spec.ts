// The Import content page's pure parts (lib/importing.ts): reading front matter, reading a file
// into items as the server reads a body, matching names to fields, putting values in the form the
// server's validation takes, cutting a list into batches, and saying a check's and a run's result.
// The shapes are the ones the server's own tests import (crates/colophon-server/tests/transfer.rs).

import { expect, test } from '@playwright/test';
import type { Definition, ImportOutcome } from '../src/lib/api.ts';
import {
	autoMap,
	batches,
	coerce,
	dateOnly,
	importable,
	isArchive,
	jsonBytes,
	kindsOf,
	mapRecord,
	markdownEntry,
	markdownFields,
	namesIn,
	normalKey,
	parseFrontMatter,
	placeOf,
	pointerInItem,
	readItems,
	refusalWords,
	runWords,
	SERVER_MAX_ITEMS,
	shapeOf,
	slugFromPath,
	suggestedKind,
	summarize,
	titleOf,
	utcTimestamp,
	wrongSource
} from '../src/lib/importing.ts';
import { INTERNAL_TERMS } from '../src/lib/words.ts';

const POSTS: Definition = {
	name: 'Posts',
	title_field: 'title',
	fields: [
		{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
		{ id: 'slug', label: 'URL name', type: 'slug', required: true, source: 'title' },
		{ id: 'pub_date', label: 'Published on', type: 'date', required: false },
		{ id: 'updated_at', label: 'Updated', type: 'datetime', required: false },
		{ id: 'summary', label: 'Summary', type: 'text', required: false, max_chars: 500, multiline: true },
		{ id: 'tags', label: 'Tags', type: 'list', required: false, of: { type: 'text', max_chars: 40, multiline: false }, max: 10 },
		{ id: 'featured', label: 'Featured', type: 'boolean', required: false },
		{ id: 'body', label: 'Body', type: 'markdown', required: false, max_chars: 100_000 }
	]
};

test.describe('front matter', () => {
	test('names and values between --- lines, and the body after them', () => {
		const read = parseFrontMatter('---\ntitle: Hello\npubDate: 2024-05-01T10:00:00Z\ntags: [a, b]\ndraft: false\n---\n\n# Hello\n\nA paragraph.\n');
		expect(read).toEqual({
			ok: true,
			found: true,
			// A timestamp stays the text it was written as: nothing here guesses a time zone.
			data: { title: 'Hello', pubDate: '2024-05-01T10:00:00Z', tags: ['a', 'b'], draft: false },
			body: '# Hello\n\nA paragraph.'
		});
	});

	test('Windows line endings and a byte-order mark read the same', () => {
		const read = parseFrontMatter('﻿---\r\ntitle: Hello\r\n---\r\nOne\r\nTwo\r\n');
		expect(read).toEqual({ ok: true, found: true, data: { title: 'Hello' }, body: 'One\nTwo' });
	});

	test('a file with no front matter is all body', () => {
		expect(parseFrontMatter('# Just a page\n\n--- is not at the top.\n')).toEqual({ ok: true, found: false, data: {}, body: '# Just a page\n\n--- is not at the top.' });
	});

	test('empty front matter, and a --- line later in the body', () => {
		expect(parseFrontMatter('---\n---\nBody\n\n---\n\nMore')).toEqual({ ok: true, found: true, data: {}, body: 'Body\n\n---\n\nMore' });
	});

	test('what can’t be read is said, not guessed at', () => {
		const toml = parseFrontMatter('+++\ntitle = "Hello"\n+++\nBody');
		expect(toml.ok).toBe(false);
		expect(toml.ok === false && toml.problem).toContain('+++');
		const open = parseFrontMatter('---\ntitle: Hello\n\nBody with no end');
		expect(open.ok === false && open.problem).toContain('no --- line to end it');
		const broken = parseFrontMatter('---\ntitle: Hello\nbad: [1, 2\n---\nBody');
		expect(broken.ok).toBe(false);
		expect(broken.ok === false && broken.problem).toMatch(/can’t be read/);
		const list = parseFrontMatter('---\n- one\n- two\n---\nBody');
		expect(list.ok === false && list.problem).toContain('names and values');
	});

	test('YAML that repeats itself many times over is refused, not expanded', () => {
		const bomb = `---\na: &a [x, x, x, x, x, x, x, x, x]\nb: &b [*a, *a, *a, *a, *a, *a, *a, *a, *a]\nc: &c [*b, *b, *b, *b, *b, *b, *b, *b, *b]\nd: [*c, *c, *c, *c, *c, *c, *c, *c, *c]\n---\n`;
		expect(parseFrontMatter(bomb).ok).toBe(false);
	});
});

test.describe('a file as items', () => {
	test('a JSON list, lines, and a Strapi answer with data beside meta', () => {
		expect(readItems('[{"title":"A"},{"title":"B"}]')).toEqual({ ok: true, items: [{ title: 'A' }, { title: 'B' }] });
		expect(readItems('{"title":"A"}\n\n{"title":"B"}\n')).toEqual({ ok: true, items: [{ title: 'A' }, { title: 'B' }] });
		expect(readItems('{"data": [{"id": 7, "attributes": {"title": "Sixth"}}], "meta": {"pagination": {"page": 1}}}')).toEqual({
			ok: true,
			items: [{ id: 7, attributes: { title: 'Sixth' } }]
		});
	});

	test('a lone data list without meta is a document, as the server reads it', () => {
		expect(readItems('{"data": [1, 2]}')).toEqual({ ok: true, items: [{ data: [1, 2] }] });
	});

	test('one entry’s answer, and one document spread over lines', () => {
		expect(readItems('{"data": {"documentId": "a1", "title": "One"}, "meta": {}}')).toEqual({ ok: true, items: [{ documentId: 'a1', title: 'One' }] });
		expect(readItems('{\n  "title": "Pretty",\n  "note": "printed"\n}')).toEqual({ ok: true, items: [{ title: 'Pretty', note: 'printed' }] });
	});

	test('an empty file, a cut-off list and a bad line each say what is wrong', () => {
		expect(readItems('  \n')).toEqual({ ok: false, problem: 'This file is empty.' });
		expect(readItems('[{"title":"A"},').ok).toBe(false);
		const bad = readItems('{"title":"A"}\n\nnot json\n');
		expect(bad).toEqual({ ok: false, problem: 'Line 3 of this file isn’t JSON. Each line should be one entry, or the whole file one JSON list.' });
	});

	test('an archive is told from text by its first bytes', () => {
		expect(isArchive(new Uint8Array([0x1f, 0x8b, 8, 0]))).toBe(true);
		expect(isArchive(new Uint8Array([0x50, 0x4b, 3, 4]))).toBe(true);
		expect(isArchive(new TextEncoder().encode('{"title":"A"}'))).toBe(false);
	});

	test('each source’s shape, as the server’s mapping tells them apart', () => {
		expect(shapeOf({ _id: 'drafts.7f1c', _type: 'notes', title: 'Seventh' })).toBe('sanity');
		expect(shapeOf({ id: 42, attributes: { title: 'Fourth' } })).toBe('strapi');
		expect(shapeOf({ documentId: 'a1b2c3', title: 'Fifth' })).toBe('strapi');
		expect(shapeOf({ type: 'api::note.note', data: { title: 'Fifth' } })).toBe('strapi');
		// A plain document may have fields called id, type or data.
		expect(shapeOf({ id: 3, title: 'Plain' })).toBe('plain');
		expect(shapeOf({ type: 'note', data: 'x' })).toBe('plain');
		expect(shapeOf('text')).toBe('plain');
	});

	test('a file that isn’t what the source says is turned away, with where it belongs', () => {
		const sanity = [{ _id: 'a', _type: 'post', title: 'A' }];
		const strapi = [{ id: 1, attributes: { title: 'A' } }];
		const plain = [{ title: 'A' }];
		expect(wrongSource(sanity, 'sanity')).toBeNull();
		expect(wrongSource(strapi, 'strapi')).toBeNull();
		expect(wrongSource(plain, 'colophon')).toBeNull();
		expect(wrongSource(plain, 'json')).toBeNull();
		expect(wrongSource(strapi, 'sanity')).toContain('Choose Strapi');
		expect(wrongSource(plain, 'sanity')).toContain('_type');
		expect(wrongSource(sanity, 'strapi')).toContain('Choose Sanity');
		expect(wrongSource(plain, 'strapi')).toContain('documentId');
		expect(wrongSource(sanity, 'colophon')).toContain('Choose Sanity');
		expect(wrongSource(strapi, 'json')).toContain('Choose Strapi');
		// A mixed file is the check's to judge, item by item.
		expect(wrongSource([...sanity, ...plain], 'sanity')).toBeNull();
	});

	test('the kinds in a mixed export, and the one to start with', () => {
		const sanity = [
			{ _id: 'p1', _type: 'post' },
			{ _id: 'p2', _type: 'post' },
			{ _id: 'a1', _type: 'author' },
			{ _id: 'i1', _type: 'sanity.imageAsset' },
			{ _id: 'i2', _type: 'sanity.imageAsset' },
			{ _id: 'i3', _type: 'sanity.imageAsset' }
		];
		const kinds = kindsOf(sanity, 'sanity');
		expect(kinds).toEqual([
			{ kind: 'sanity.imageAsset', count: 3 },
			{ kind: 'post', count: 2 },
			{ kind: 'author', count: 1 }
		]);
		expect(suggestedKind(kinds, 'posts')).toBe('post');
		expect(suggestedKind(kinds, 'authors')).toBe('author');
		// Nothing named like it: the most common that isn't the source's own bookkeeping.
		expect(suggestedKind(kinds, 'articles')).toBe('post');
		const strapi = kindsOf(
			[
				{ type: 'plugin::upload.file', data: {} },
				{ type: 'plugin::upload.file', data: {} },
				{ type: 'api::article.article', data: {} }
			],
			'strapi'
		);
		expect(suggestedKind(strapi, 'articles')).toBe('api::article.article');
		// A REST answer names no kind: there is nothing to choose.
		expect(kindsOf([{ id: 1, attributes: {} }], 'strapi')).toEqual([]);
		expect(suggestedKind([], 'articles')).toBeNull();
	});

	test('an item’s title is found where each source keeps it', () => {
		expect(titleOf({ title: ' Plain ' }, 'title')).toBe('Plain');
		expect(titleOf({ id: 1, attributes: { title: 'Nested' } }, 'title')).toBe('Nested');
		expect(titleOf({ type: 'api::a.a', data: { title: 'Export' } }, 'title')).toBe('Export');
		expect(titleOf({ note: 'none' }, 'title')).toBe('');
	});
});

test.describe('names to fields', () => {
	test('one name whatever its case, hyphens and underscores', () => {
		expect(normalKey('pubDate')).toBe(normalKey('pub_date'));
		expect(normalKey('Pub-Date')).toBe('pubdate');
	});

	test('exact names first, then the same name written another way; the rest are left out', () => {
		const names = namesIn([{ title: 'A', pubDate: '2024-05-01', heroImage: './a.jpg' }, { title: 'B', Tags: ['x'], draft: true }]);
		expect(names).toEqual(['title', 'pubDate', 'heroImage', 'Tags', 'draft']);
		expect(autoMap(names, POSTS.fields)).toEqual({ title: 'title', pubDate: 'pub_date', heroImage: null, Tags: 'tags', draft: null });
	});

	test('an exact name keeps its field when another spelling of it comes first', () => {
		expect(autoMap(['PubDate', 'pub_date'], POSTS.fields)).toEqual({ PubDate: null, pub_date: 'pub_date' });
	});

	test('the field the body fills is never offered to front matter', () => {
		expect(autoMap(['body', 'title'], POSTS.fields, ['body'])).toEqual({ body: null, title: 'title' });
	});

	test('dates take the form each field stores', () => {
		expect(dateOnly('2024-05-01T10:00:00Z')).toBe('2024-05-01');
		expect(dateOnly('2024-05-01 10:00')).toBe('2024-05-01');
		expect(dateOnly('2024-05-01')).toBe('2024-05-01');
		// Not a date: left for the server to refuse.
		expect(dateOnly('May 1st')).toBe('May 1st');
		expect(utcTimestamp('2024-05-01')).toBe('2024-05-01T00:00:00Z');
		expect(utcTimestamp('2024-05-01T10:00:00.123Z')).toBe('2024-05-01T10:00:00Z');
		expect(utcTimestamp('2024-05-01T10:00:00+02:00')).toBe('2024-05-01T08:00:00Z');
		expect(utcTimestamp('2024-05-01 10:00')).toBe('2024-05-01T10:00:00Z');
		expect(utcTimestamp('2024-02-31T10:00:00Z')).toBe('2024-02-31T10:00:00Z');
		expect(utcTimestamp('yesterday')).toBe('yesterday');
	});

	test('values take the form the server’s validation asks for, and nothing empty is sent', () => {
		const text = { type: 'text', multiline: false };
		expect(coerce('  Hello  ', text)).toBe('Hello');
		expect(coerce(1984, text)).toBe('1984');
		expect(coerce('', text)).toBeUndefined();
		expect(coerce('   ', text)).toBeUndefined();
		expect(coerce(null, text)).toBeUndefined();
		expect(coerce('one\r\ntwo', { type: 'text', multiline: true })).toBe('one\ntwo');
		expect(coerce('a\r\nb', { type: 'markdown' })).toBe('a\nb');
		expect(coerce('12.5', { type: 'number' })).toBe(12.5);
		expect(coerce('twelve', { type: 'number' })).toBe('twelve');
		expect(coerce('true', { type: 'boolean' })).toBe(true);
		expect(coerce(['  a ', '', 'b'], { type: 'list', of: { type: 'text' } })).toEqual(['a', 'b']);
		// Structures and links go as they are: the server decides.
		expect(coerce({ _ref: 'x' }, { type: 'reference' })).toEqual({ _ref: 'x' });
	});

	test('a record becomes a document by its mapping', () => {
		const record = { title: ' Hello ', pubDate: '2024-05-01T10:00:00Z', heroImage: './a.jpg', Tags: ['x'], featured: 'true', summary: '' };
		const mapping = autoMap(Object.keys(record), POSTS.fields);
		expect(mapRecord(record, mapping, POSTS.fields)).toEqual({ title: 'Hello', pub_date: '2024-05-01', tags: ['x'], featured: true });
		// The person may point a name at another field, or leave it out.
		expect(mapRecord(record, { ...mapping, title: null, heroImage: 'summary' }, POSTS.fields)).toEqual({ summary: './a.jpg', pub_date: '2024-05-01', tags: ['x'], featured: true });
	});

	test('a URL name from a file’s name', () => {
		expect(slugFromPath('posts/Hello World.md')).toBe('hello-world');
		expect(slugFromPath('2024-05-01-first-post.markdown')).toBe('2024-05-01-first-post');
		expect(slugFromPath('blog/hello-world/index.md')).toBe('hello-world');
		expect(slugFromPath('index.md')).toBe('index');
		expect(slugFromPath('Crème brûlée.mdx')).toBe('creme-brulee');
	});

	test('a Markdown file becomes its entry: front matter mapped, body in the Markdown field, URL name from the file', () => {
		expect(markdownFields(POSTS).map((field) => field.id)).toEqual(['body']);
		const front = parseFrontMatter('---\ntitle: Hello\npubDate: 2024-05-01T10:00:00Z\nbody: not this\n---\n# Hello\r\n\r\nText.\n');
		if (!front.ok) throw new Error(front.problem);
		const mapping = autoMap(Object.keys(front.data), POSTS.fields, ['body']);
		expect(markdownEntry('posts/hello-there.md', front, mapping, POSTS, 'body')).toEqual({
			title: 'Hello',
			pub_date: '2024-05-01',
			body: '# Hello\n\nText.',
			slug: 'hello-there'
		});
		// The front matter's own URL name wins; an empty body is left out.
		const named = parseFrontMatter('---\ntitle: Hello\nslug: my-own\n---\n');
		if (!named.ok) throw new Error(named.problem);
		expect(markdownEntry('x.md', named, autoMap(Object.keys(named.data), POSTS.fields, ['body']), POSTS, 'body')).toEqual({ title: 'Hello', slug: 'my-own' });
	});
});

test.describe('batches', () => {
	test('in order, at most so many in each', () => {
		const items = Array.from({ length: 250 }, (_, index) => ({ n: index }));
		const cut = batches(items, 100);
		expect(cut.map((batch) => batch.length)).toEqual([100, 100, 50]);
		expect(cut.flat()).toEqual(items);
		expect(batches([], 100)).toEqual([]);
	});

	test('never more than the server takes in one import, whatever is asked for', () => {
		const items = Array.from({ length: 1100 }, (_, index) => index);
		const cut = batches(items, 5000);
		expect(cut.map((batch) => batch.length)).toEqual([SERVER_MAX_ITEMS, SERVER_MAX_ITEMS, 1100 - 2 * SERVER_MAX_ITEMS]);
	});

	test('a batch also ends before it is too many bytes; one large item goes alone', () => {
		const big = { text: 'x'.repeat(800) };
		const small = { text: 'y' };
		const cut = batches([small, big, big, small, { text: 'z'.repeat(5000) }, small], 100, 1500);
		expect(cut.map((batch) => batch.length)).toEqual([2, 2, 1, 1]);
		for (const batch of cut.slice(0, 2)) expect(batch.reduce((sum, item) => sum + jsonBytes(item) + 1, 0)).toBeLessThanOrEqual(1500);
		// Bytes, not characters.
		expect(jsonBytes('é')).toBe(4);
	});
});

test.describe('a check’s result', () => {
	const outcomes: ImportOutcome[] = [
		{ item: '/0', status: 'would_create', entry: 'e-0', source: '42', unmapped: ['createdAt', 'publishedAt'] },
		{ item: '/1', status: 'would_update', entry: 'e-1', source: 'e-1', unmapped: ['createdAt', 'locale'] },
		{ item: '/2', status: 'skipped', reason: 'its `_type` is `author`, not `post`, which `?type=` asked for' },
		{ item: '/3', status: 'invalid', refusal: { code: 'missing_field', pointer: '/3/title', message: '`Title` is required' } },
		{ item: '/4', status: 'invalid', refusal: { code: 'unknown_field', pointer: '/4/heroImage', message: '`heroImage` is not allowed here' } },
		{ item: '/5', status: 'invalid', refusal: { code: 'slug_taken', pointer: '/5/slug', message: 'another entry already uses this slug' } },
		{ item: '/6', status: 'invalid', refusal: { code: 'missing_permission', pointer: '', message: "your role can't create entries here" } },
		{ item: '/7', status: 'invalid', refusal: { code: 'invalid_date', pointer: '/7/pub_date', message: 'Published on is a date like 2026-10-28' } },
		{ item: '/8', status: 'would_create', entry: 'e-8' }
	];

	test('how many will be created, updated, skipped, and can’t be imported', () => {
		const summary = summarize(outcomes, POSTS);
		expect({ create: summary.create, update: summary.update, skipped: summary.skipped, invalid: summary.invalid }).toEqual({ create: 2, update: 1, skipped: 1, invalid: 5 });
		expect(summary.setAside).toEqual(['createdAt', 'locale', 'publishedAt']);
		expect(importable(outcomes)).toEqual([0, 1, 8]);
	});

	test('each one that can’t, with its reason in plain words and the field it points at', () => {
		const { problems } = summarize(outcomes, POSTS);
		expect(problems.map(({ index, field, text }) => ({ index, field, text }))).toEqual([
			{ index: 3, field: 'Title', text: 'Title is required.' },
			{ index: 4, field: null, text: 'It has “heroImage”, and this content type has no field with that name.' },
			{ index: 5, field: 'URL name', text: 'Another entry already uses this URL name.' },
			{ index: 6, field: null, text: 'Your role can’t create or change this entry.' },
			{ index: 7, field: 'Published on', text: 'Published on is a date like 2026-10-28.' }
		]);
		// No internal word reaches the page; the server's own stay in the Technical details.
		for (const problem of problems) expect(problem.text).not.toMatch(INTERNAL_TERMS);
		expect(problems[2]?.technical).toBe('slug_taken · /5/slug · another entry already uses this slug');
	});

	test('a pointer is read inside its item, wherever the item sits', () => {
		expect(pointerInItem('/3/title', '/3')).toBe('/title');
		expect(pointerInItem('/line/3/sections/2/byline', '/line/3')).toBe('/sections/2/byline');
		expect(pointerInItem('/locale', '/3')).toBe('/locale');
		expect(pointerInItem('/30/title', '/3')).toBe('/30/title');
		expect(placeOf('/12/title')).toEqual({ index: 12, rest: '/title' });
		expect(placeOf('/12')).toEqual({ index: 12, rest: '' });
		expect(placeOf('/')).toBeNull();
		expect(refusalWords({ code: 'too_long', pointer: '/sections/2/byline', message: 'Byline is at most 80 characters' }, { fields: [{ id: 'sections', label: 'Sections', type: 'json', required: false }] })).toEqual({
			field: 'Sections',
			text: 'Byline is at most 80 characters.'
		});
	});

	test('a server message with an internal word in it is replaced, not shown', () => {
		expect(refusalWords({ code: 'invalid_slug', pointer: '/slug', message: 'a slug is lowercase' }, POSTS)).toEqual({ field: 'URL name', text: 'Colophon can’t take this entry as it is.' });
	});
});

test.describe('a run’s result', () => {
	test('every batch answered: imported, as drafts', () => {
		expect(runWords({ sizes: [100, 100, 20], done: 3, stopped: null })).toEqual({
			ok: true,
			title: 'Imported 220 entries',
			text: 'They are drafts. Nothing is published until you publish it.'
		});
		expect(runWords({ sizes: [1], done: 1, stopped: null }).title).toBe('Imported 1 entry');
	});

	test('a refused batch after others landed is a part, never a success', () => {
		const said = runWords({ sizes: [100, 100, 100, 20], done: 2, stopped: { batch: 2, how: 'refused' } });
		expect(said.ok).toBe(false);
		expect(said.title).toBe('Imported 200 of 320 entries');
		expect(said.text).toBe(
			'Batch 3 of 4 (entries 201 to 300) was refused, and none of that batch was imported. The 200 entries before it are in, as drafts. The 20 entries after it were not sent.'
		);
	});

	test('a refused first batch imported nothing', () => {
		expect(runWords({ sizes: [100, 20], done: 0, stopped: { batch: 0, how: 'refused' } })).toEqual({
			ok: false,
			title: 'Nothing was imported',
			text: 'Batch 1 of 2 (entries 1 to 100) was refused, and none of that batch was imported. Nothing before it was imported. The 20 entries after it were not sent.'
		});
		expect(runWords({ sizes: [12], done: 0, stopped: { batch: 0, how: 'refused' } })).toEqual({
			ok: false,
			title: 'Nothing was imported',
			text: 'The import was refused, so none of the 12 entries were imported.'
		});
		expect(runWords({ sizes: [1], done: 0, stopped: { batch: 0, how: 'unanswered' } }).text).toBe(
			'Colophon didn’t answer, so it isn’t known whether the 1 entry was imported. Try again: the same entries are never added twice.'
		);
	});

	test('a batch with no answer is unknown, and says trying again is safe', () => {
		const said = runWords({ sizes: [100, 100, 1], done: 1, stopped: { batch: 1, how: 'unanswered' } });
		expect(said.ok).toBe(false);
		expect(said.title).toBe('Imported 100 of 201 entries so far');
		expect(said.text).toBe(
			'Batch 2 of 3 (entries 101 to 200) got no answer, so it isn’t known whether those were imported. The 100 entries before it are in, as drafts. The 1 entry after it was not sent. Try again: the same batch is never added twice.'
		);
	});
});
