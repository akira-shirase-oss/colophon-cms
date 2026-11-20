// Key helpers shared by k6 scenarios, built on idempotency-key.js. Pure ES module, no k6 imports, so node tests
// them (keys.test.mjs).
import { idempotencyKey } from './idempotency-key.js';

/**
 * The keys of a race of `n` concurrent requests on one base key, built once: send `keys[i]` with request i, and
 * replay the winner with `keys[winner]`. Sender and replayer then share one builder call, so they can't drift
 * apart (undo's replay sent an unseen key, CI 44698136206).
 */
export function raceKeys(base, n, scenario) {
  return Array.from({ length: n }, (_, i) => idempotencyKey(`${base}-r${i + 1}`, scenario));
}

/**
 * A key unique to this VU and this call: `vu` in the key, so two VUs in the same millisecond with the same
 * per-VU `sequence` can't collide (bulk's did, and with differing bodies the server refused the second as
 * idempotency_key_reused, CI 44698136206).
 */
export function sequenceKey(prefix, vu, now, sequence, scenario) {
  return idempotencyKey(`${prefix}-v${vu}-${now.toString(36)}-${sequence}`, scenario);
}
