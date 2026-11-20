// The k6 scenarios' Idempotency-Key builder against the API's rule, at its boundaries:
//   node --test perf/k6/lib/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { idempotencyKey, valid, MIN_LENGTH, MAX_LENGTH } from './idempotency-key.js';

test('the length bounds are the API\'s, inclusive', () => {
  assert.equal(MIN_LENGTH, 16);
  assert.equal(MAX_LENGTH, 128);
  assert.equal(valid('a'.repeat(15)), false);
  assert.equal(valid('a'.repeat(16)), true);
  assert.equal(valid('a'.repeat(128)), true);
  assert.equal(valid('a'.repeat(129)), false);
});

test('the alphabet is letters, digits, - and _ only', () => {
  assert.equal(valid('ABCxyz0189-_abcd'), true);
  for (const bad of ['.', ' ', '/', ':', '+', '=', 'é', '\u0000']) {
    assert.equal(valid(`k6-undo-abcdefgh${bad}`), false, `accepts ${JSON.stringify(bad)}`);
  }
  assert.equal(valid(''), false);
  assert.equal(valid(undefined), false);
  assert.equal(valid(1234567890123456), false, 'only strings');
});

test('idempotencyKey joins parts with "-" and returns a valid key', () => {
  assert.equal(idempotencyKey('k6', 'undo', 'mwo3rkbh', 0, 'entry'), 'k6-undo-mwo3rkbh-0-entry');
  assert.equal(idempotencyKey('a'.repeat(16)), 'a'.repeat(16));
  assert.equal(idempotencyKey('a'.repeat(128)), 'a'.repeat(128));
});

test('idempotencyKey throws, naming the key, outside the rule', () => {
  // The undo smoke's old setup key: 15 characters.
  assert.throws(() => idempotencyKey('k6', 'mwo3rkbh', 0, 'e'), /"k6-mwo3rkbh-0-e" \(15 bytes\)/);
  assert.throws(() => idempotencyKey('a'.repeat(129)), /129 bytes/);
  assert.throws(() => idempotencyKey('k6.undo.abcdefghij'), /outside the API's rule/);
});

test('every key the undo smoke builds is valid for any run stamp and any VU and iteration', () => {
  // undo.js's keys, with the shortest run stamp Date.now().toString(36) can be today (8) and a long one.
  for (const run of ['mwo3rkbh', 'z'.repeat(12)]) {
    for (const [vu, iter, i] of [[1, 0, 0], [512, 99999, 49]]) {
      const key = idempotencyKey('k6', 'undo', run, vu, iter);
      for (const suffix of ['c', 'p', 'u1', 'u2']) assert.ok(valid(`${key}-${suffix}`), `${key}-${suffix}`);
      assert.ok(valid(idempotencyKey('k6', 'undo', run, i, 'entry')));
      const first = idempotencyKey('k6', 'undo', run, i, 'first');
      for (const suffix of ['c', 'p']) assert.ok(valid(`${first}-${suffix}`));
    }
  }
});
