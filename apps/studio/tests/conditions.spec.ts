// Conditional fields (D-072) in the Studio: the rule that decides whether a field applies, the same
// as the server's (`Condition::holds`), and the words for its refusal and for a cleared value.

import { expect, test } from '@playwright/test';
import type { FieldDefinition } from '../src/lib/api.ts';
import { applies } from '../src/lib/content.ts';
import { clearedNote, INTERNAL_TERMS, INVALID_CONDITION } from '../src/lib/words.ts';

const duration = { id: 'duration', label: 'Duration', required: true, type: 'number', integer: true, when: { field: 'kind', is: ['video', 'podcast'] } } as FieldDefinition;
const badge = { id: 'badge', label: 'Badge', required: false, type: 'text', max_chars: 20, multiline: false, when: { field: 'featured', is: [true] } } as FieldDefinition;

test('a field applies while the field it depends on holds one of its values, and not when it is absent', () => {
	expect(applies(duration, { kind: 'video' })).toBe(true);
	expect(applies(duration, { kind: 'podcast' })).toBe(true);
	expect(applies(duration, { kind: 'article' })).toBe(false);
	expect(applies(duration, {})).toBe(false);
	expect(applies(badge, { featured: true })).toBe(true);
	expect(applies(badge, { featured: false })).toBe(false);
	expect(applies(badge, { featured: 'true' })).toBe(false);
	const always = { id: 'title', label: 'Title', required: true, type: 'text', max_chars: 200, multiline: false } as FieldDefinition;
	expect(applies(always, {})).toBe(true);
});

test('the words are plain', () => {
	expect(clearedNote('Duration', 'Kind', ['video', 'podcast'])).toBe('Duration is cleared because it only applies when Kind is video or podcast.');
	for (const words of [INVALID_CONDITION, clearedNote('Badge', 'Featured', ['Yes'])]) expect(INTERNAL_TERMS.test(words), words).toBe(false);
});
