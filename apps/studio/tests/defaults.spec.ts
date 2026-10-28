// Field defaults (D-071) in the Studio: which fields may have one, the control each is chosen with
// in the builder (the field's own kind of input), and their words. A new entry leaves defaulted
// fields to the server, which fills them on creation.

import { expect, test } from '@playwright/test';
import type { Definition, FieldDefinition } from '../src/lib/api.ts';
import { starter } from '../src/lib/content.ts';
import { canHaveDefault, defaultInput } from '../src/lib/fieldDefs.ts';
import { DEFAULT_VALUE, INTERNAL_TERMS, INVALID_DEFAULT } from '../src/lib/words.ts';

const field = (rest: Record<string, unknown>) => ({ id: 'x', label: 'X', required: false, ...rest }) as FieldDefinition;

test('each kind of field chooses its default with its own kind of input', () => {
	const cases: [Record<string, unknown>, ReturnType<typeof defaultInput>][] = [
		[{ type: 'text', max_chars: 200, multiline: false }, 'text'],
		[{ type: 'text', max_chars: 200, multiline: true }, 'textarea'],
		[{ type: 'markdown', max_chars: 1000 }, 'textarea'],
		[{ type: 'number', integer: false }, 'number'],
		[{ type: 'boolean' }, 'yes-no'],
		[{ type: 'enum', values: ['a', 'b'] }, 'choice'],
		[{ type: 'date' }, 'date'],
		[{ type: 'time' }, 'time'],
		[{ type: 'datetime' }, 'datetime-local'],
		[{ type: 'email' }, 'email'],
		[{ type: 'url' }, 'url'],
		[{ type: 'color' }, 'color'],
		[{ type: 'json' }, null],
		[{ type: 'rich_text' }, null],
		[{ type: 'reference', to: 'people', many: false }, null]
	];
	for (const [rest, input] of cases) expect(defaultInput(field(rest)), String(rest['type'])).toBe(input);
});

test('only top-level fields that aren’t unique, of a kind with one sensible start, may have a default', () => {
	expect(canHaveDefault(field({ type: 'text', max_chars: 200, multiline: false }), 0)).toBe(true);
	expect(canHaveDefault(field({ type: 'text', max_chars: 200, multiline: false, unique: true }), 0)).toBe(false);
	expect(canHaveDefault(field({ type: 'email' }), 1)).toBe(false);
	for (const type of ['json', 'rich_text', 'reference', 'slug', 'object', 'list', 'blocks']) expect(canHaveDefault(field({ type }), 0), type).toBe(false);
});

test('a new entry leaves a defaulted field to the server, instead of the first choice or false', () => {
	const definition: Definition = {
		name: 'Things',
		title_field: 'title',
		fields: [
			field({ id: 'status', label: 'Status', required: true, type: 'enum', values: ['draft', 'review'], default: 'review' }),
			field({ id: 'kind', label: 'Kind', required: true, type: 'enum', values: ['a', 'b'] }),
			field({ id: 'featured', label: 'Featured', required: true, type: 'boolean', default: true })
		]
	};
	expect(starter(definition)).toEqual({ kind: 'a' });
});

test('the words are plain', () => {
	for (const words of [DEFAULT_VALUE, INVALID_DEFAULT]) expect(INTERNAL_TERMS.test(words), words).toBe(false);
});
