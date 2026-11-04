// The same wire, once against the real binary — the `#[ignore]` equivalent for TypeScript:
// gated on `COLOPHON_TEST_ORIGIN` (scripts/local-env.sh sets it for the test stack; the
// package's `npm test` skips this file entirely without it). Unauthenticated only: what a
// client can always do — health, version, and a 401 that arrives in the envelope.

/// <reference types="node" />
import { describe, expect, it } from 'vitest';
import { ApiError, Client } from '../src/index.ts';

const ORIGIN = process.env['COLOPHON_TEST_ORIGIN'];

describe.skipIf(ORIGIN === undefined)('the client against the real server', () => {
	const client = new Client({ baseUrl: ORIGIN ?? 'http://invalid' });

	it('answers /version in the documented shape', async () => {
		const reply = await client.version();
		expect(reply.response.status).toBe(200);
		expect(reply.version).toMatch(/^\d+\.\d+\.\d+/);
		expect(reply.api).toContain('v1');
		expect(typeof reply.commit).toBe('string');
	});

	it('answers /livez', async () => {
		const reply = await client.livez();
		expect(reply.response.status).toBe(200);
	});

	it('reads /me into a typed 401, envelope and request id included', async () => {
		try {
			await client.me();
			throw new Error('unauthenticated /me should fail');
		} catch (error) {
			expect(error).toBeInstanceOf(ApiError);
			const api = error as ApiError;
			expect(api.status).toBe(401);
			expect(api.code).toBe('unauthorized');
			expect(api.requestId).toMatch(/^[0-9a-f-]{36}$/);
			expect(api.retryable).toBe(false);
		}
	});

	it('serves the OpenAPI document the types were generated from', async () => {
		// Deliberately unwrapped: the byte-compared artifact is docs/api/openapi.json; this
		// just proves the endpoint the SDK's types mirror is really there.
		const response = await fetch(`${ORIGIN}/openapi.json`);
		expect(response.status).toBe(200);
		const doc = (await response.json()) as { openapi: string; paths: Record<string, unknown> };
		expect(doc.openapi).toBe('3.1.0');
		expect(Object.keys(doc.paths)).toContain('/api/v1/projects/{project}/environments/{environment}/collections/{collection}/entries');
	});
});
