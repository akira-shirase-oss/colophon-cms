// Unique fields (D-070) in the Studio's words: a value another entry has, said with the field's
// name; the publish flow's and the builder's refusals said plainly; no internal terms.

import { expect, test } from '@playwright/test';
import { ApiError, type FieldDefinition } from '../src/lib/api.ts';
import { describe } from '../src/lib/errors.ts';
import { canBeUnique } from '../src/lib/fieldDefs.ts';
import { INTERNAL_TERMS, INVALID_UNIQUE, UNIQUE, VALUE_TAKEN_ON_PUBLISH, valueTaken } from '../src/lib/words.ts';

test('a taken value is said with the field’s name', () => {
	expect(valueTaken('Email')).toBe('Another entry already has this Email.');
});

test('the refusals read plainly through the shared error words', () => {
	const invalid = new ApiError('refused', 'only a top-level single-line text … can be unique', 422, 'invalid_unique', null, {
		reason: 'invalid_unique',
		pointer: '/fields/2/unique'
	});
	expect(describe(invalid).text).toBe(INVALID_UNIQUE);
	const taken = new ApiError('refused', 'another entry already has this Email (draft), and it must be unique', 409, 'value_taken', null, {
		reason: 'value_taken',
		pointer: '/email'
	});
	expect(describe(taken).text).toMatch(/^Another entry already has this value/);
	for (const words of [UNIQUE, INVALID_UNIQUE, VALUE_TAKEN_ON_PUBLISH, valueTaken('Email'), describe(taken).text]) {
		expect(INTERNAL_TERMS.test(words), words).toBe(false);
	}
});

test('only top-level one-line text, email, web address, number, date, time and date-and-time fields can be unique', () => {
	const field = (rest: Record<string, unknown>) => ({ id: 'x', label: 'X', required: false, ...rest }) as FieldDefinition;
	expect(canBeUnique(field({ type: 'text', max_chars: 200, multiline: false }), 0)).toBe(true);
	expect(canBeUnique(field({ type: 'text', max_chars: 200, multiline: true }), 0)).toBe(false);
	for (const type of ['email', 'url', 'number', 'date', 'time', 'datetime']) expect(canBeUnique(field({ type }), 0), type).toBe(true);
	for (const type of ['markdown', 'json', 'color', 'boolean', 'rich_text']) expect(canBeUnique(field({ type }), 0), type).toBe(false);
	expect(canBeUnique(field({ type: 'email' }), 1)).toBe(false);
});
