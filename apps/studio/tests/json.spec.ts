// The Studio's JSON canonical form (lib/json.ts) against the server's (`write_json`, D-064): object
// keys sorted by UTF-16 code units at every depth (so an astral key sorts before U+FF71, where
// UTF-8 byte order would put it after), strings escaped as JSON.stringify does, -0 written 0,
// decimals as written. The expected bytes are the server's: tests/server/fields.spec.ts stores
// the same value through the real API and compares what the server keeps with this function.

import { expect, test } from '@playwright/test';
import { canonicalJson, JsonProblem, prettyJson, readJson } from '../src/lib/json.ts';
import { TRICKY, TRICKY_CANONICAL } from './json-fixture.ts';

test('a tricky value is written in the server’s canonical form', () => {
	expect(canonicalJson(TRICKY)).toBe(TRICKY_CANONICAL);
	// Whatever order it arrives in, the same value writes the same bytes.
	expect(canonicalJson(JSON.parse(JSON.stringify(TRICKY)))).toBe(TRICKY_CANONICAL);
	expect(canonicalJson(JSON.parse(TRICKY_CANONICAL))).toBe(TRICKY_CANONICAL);
});

test('numbers follow number-field rules, as the server’s are', () => {
	expect(canonicalJson([1.0, 1e2, 0.1, 123456789.123456, -0.000001])).toBe('[1,100,0.1,123456789.123456,-0.000001]');
	for (const refused of [1e30, 0.1234567, 1_000_000_000.5, Number.POSITIVE_INFINITY, 9_007_199_254_740_992]) {
		expect(() => canonicalJson({ x: refused }), String(refused)).toThrow(JsonProblem);
	}
});

test('depth and size have the server’s limits', () => {
	let deep: unknown = 1;
	for (let level = 0; level < 32; level += 1) deep = [deep];
	expect(() => canonicalJson(deep)).not.toThrow();
	expect(() => canonicalJson([deep])).toThrow(/32 levels/);
	expect(() => canonicalJson('x'.repeat(64 * 1024))).toThrow(/64 KiB/);
});

test('text the editor reads: problems in words, values canonical, shown indented', () => {
	expect(readJson('{"b": 1, "a": [1.5, -0]}')).toEqual({ value: { a: [1.5, 0], b: 1 } });
	expect(readJson('{"a":').problem).toMatch(/^This isn't valid JSON yet/);
	expect(readJson('{"x": 1e30}').problem).toBe('A whole number between -9007199254740991 and 9007199254740991.');
	expect(prettyJson({ b: 1, a: 2 })).toBe('{\n  "a": 2,\n  "b": 1\n}');
});
