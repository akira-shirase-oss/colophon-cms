// An Idempotency-Key the API accepts, built from a scenario's parts. The API's rule
// (crates/colophon-api/src/entries.rs, `idempotency_key`): 16 to 128 bytes, each an ASCII letter, a digit,
// '-' or '_'. A key outside it is refused 400 bad_request, so a load scenario that builds one by hand can
// fail in setup before it measures anything (the undo smoke did, CI 44617695003, with a 15-character key).
//
// Pure ES module, no k6 imports: k6 scenarios import it, and node tests it (idempotency-key.test.mjs).

export const MIN_LENGTH = 16;
export const MAX_LENGTH = 128;
const ALLOWED = /^[A-Za-z0-9_-]+$/;

/** Whether the API would accept `key`. */
export function valid(key) {
  return typeof key === 'string' && key.length >= MIN_LENGTH && key.length <= MAX_LENGTH && ALLOWED.test(key);
}

/**
 * `parts` joined with '-'. Throws, naming the key, if the result is outside the API's rule: a scenario
 * should fail loudly at the line that built a bad key, not as a 400 in the middle of a run.
 */
export function idempotencyKey(...parts) {
  const key = parts.map(String).join('-');
  if (!valid(key)) {
    throw new Error(`Idempotency-Key ${JSON.stringify(key)} (${key.length} bytes) is outside the API's rule: ${MIN_LENGTH} to ${MAX_LENGTH} letters, digits, '-' or '_'`);
  }
  return key;
}
