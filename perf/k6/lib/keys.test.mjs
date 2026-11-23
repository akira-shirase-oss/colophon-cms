// The scenarios' race and sequence keys: node --test perf/k6/lib/*.test.mjs
import { test } from 'node:test';
import assert from 'node:assert/strict';
import { raceKeys, sequenceKey } from './keys.js';

test('a race replays with the key its winner sent, built by the same call', () => {
  const keys = raceKeys('k6-undo-run1-3-7', 2, 'undo');
  assert.equal(keys.length, 2);
  assert.notEqual(keys[0], keys[1]);
  // The replay picks from the same array the requests were sent with: one builder call, one value.
  for (const winner of [0, 1]) assert.equal(keys[winner], raceKeys('k6-undo-run1-3-7', 2, 'undo')[winner]);
});

test('two VUs in the same millisecond with the same sequence get different keys', () => {
  const now = 1763625600000;
  assert.notEqual(sequenceKey('k6-bulk', 1, now, 5, 'bulk'), sequenceKey('k6-bulk', 2, now, 5, 'bulk'));
  // The old key had no VU in it: the collision the VU removes.
  const old = (sequence) => `k6-bulk-${now.toString(36)}-${sequence}`;
  assert.equal(old(5), old(5));
});

test('within one VU, the sequence keeps keys apart in the same millisecond', () => {
  const now = 1763625600000;
  assert.notEqual(sequenceKey('k6-bulk', 1, now, 5, 'bulk'), sequenceKey('k6-bulk', 1, now, 6, 'bulk'));
});
