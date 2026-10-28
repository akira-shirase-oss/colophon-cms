// A JSON field's value in its one canonical form (D-064), byte for byte what the server stores
// (`write_json` in crates/colophon-domain/src/schema.rs): object keys sorted by UTF-16 code units
// at every depth (JavaScript's default sort), strings as `JSON.stringify` escapes them, and numbers
// as number fields write them: whole numbers within ±(2^53 − 1), other numbers below 10^9 with at
// most six decimal places, `-0` as `0`. At most 32 levels deep and 64 KiB written. The editor
// compares versions and keeps device copies with it, so a value read back from the server is the
// same edit as the one sent.

export const MAX_JSON_BYTES = 64 * 1024;
export const MAX_JSON_DEPTH = 32;
const MAX_SAFE = 9_007_199_254_740_991;
const MAX_DECIMAL = 1e9;
const PLACES = 1e6;

/** Why a value can't be stored, in the server's words: `invalid_json` or `too_long`. */
export class JsonProblem extends Error {
	constructor(
		readonly code: 'invalid_json' | 'too_long',
		message: string
	) {
		super(message);
	}
}

function number(value: number): string {
	if (!Number.isFinite(value)) throw new JsonProblem('invalid_json', 'numbers must be finite');
	if (Number.isInteger(value)) {
		if (Math.abs(value) > MAX_SAFE) throw new JsonProblem('invalid_json', `a whole number between -${MAX_SAFE} and ${MAX_SAFE}`);
	} else {
		if (Math.abs(value) >= MAX_DECIMAL) throw new JsonProblem('invalid_json', 'decimals stay below 1,000,000,000 in size');
		if (Math.round(value * PLACES) / PLACES !== value) throw new JsonProblem('invalid_json', 'at most 6 decimal places');
	}
	// JSON.stringify writes -0 as 0, and in this range the shortest round-trip spelling, as the
	// server's six-place trimmed form does.
	return JSON.stringify(value);
}

function write(value: unknown, depth: number): string {
	if (depth > MAX_JSON_DEPTH) throw new JsonProblem('invalid_json', `JSON nests at most ${MAX_JSON_DEPTH} levels deep`);
	if (value === null) return 'null';
	switch (typeof value) {
		case 'boolean':
			return value ? 'true' : 'false';
		case 'number':
			return number(value);
		case 'string':
			return JSON.stringify(value);
		case 'object': {
			if (Array.isArray(value)) return `[${value.map((item) => write(item, depth + 1)).join(',')}]`;
			const object = value as Record<string, unknown>;
			const keys = Object.keys(object).sort();
			return `{${keys.map((key) => `${JSON.stringify(key)}:${write(object[key], depth + 1)}`).join(',')}}`;
		}
		default:
			throw new JsonProblem('invalid_json', 'only JSON values can be stored');
	}
}

/** `value` written canonically; throws a `JsonProblem` for what the server would refuse. */
export function canonicalJson(value: unknown): string {
	const written = write(value, 0);
	if (new TextEncoder().encode(written).length > MAX_JSON_BYTES) {
		throw new JsonProblem('too_long', `at most ${MAX_JSON_BYTES / 1024} KiB of JSON`);
	}
	return written;
}

/** What a JSON field's text holds: its canonical value, or why it can't be stored yet. */
export function readJson(text: string): { value: unknown; problem?: undefined } | { value?: undefined; problem: string } {
	let parsed: unknown;
	try {
		parsed = JSON.parse(text);
	} catch (error) {
		return { problem: `This isn't valid JSON yet: ${(error as Error).message}` };
	}
	try {
		return { value: JSON.parse(canonicalJson(parsed)) as unknown };
	} catch (error) {
		return { problem: error instanceof JsonProblem ? `${error.message.slice(0, 1).toUpperCase()}${error.message.slice(1)}.` : String(error) };
	}
}

/** A stored value as people read it: canonical key order, indented. */
export function prettyJson(value: unknown): string {
	try {
		return JSON.stringify(JSON.parse(canonicalJson(value)), null, 2);
	} catch {
		return JSON.stringify(value, null, 2);
	}
}
