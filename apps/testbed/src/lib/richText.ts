// Sends a rich-text document to the server's validator and compares the verdict with what a
// shared fixture expects.

import { call, envelopeOf, type Reply } from './api.ts';
import type { RichTextFixture } from './fixtures.ts';

export interface Verdict {
	pass: boolean;
	detail: string;
	reply: Reply;
}

export function validate(document: string): Promise<Reply> {
	return call('POST', '/api/v1/rich-text/validate', {
		headers: { 'Content-Type': 'application/json' },
		body: document
	});
}

export async function check(fixture: RichTextFixture): Promise<Verdict> {
	const reply = await validate(JSON.stringify(fixture.value));
	if (fixture.kind === 'valid') {
		const value = (reply.body as { value?: unknown } | null)?.value;
		if (reply.status !== 200) return { pass: false, detail: `expected 200, got ${reply.status}`, reply };
		const same = JSON.stringify(value) === JSON.stringify(fixture.value);
		return {
			pass: same,
			detail: same ? `accepted, canonical form identical (${reply.ms.toFixed(1)} ms)` : 'accepted, but the canonical form differs',
			reply
		};
	}
	const envelope = envelopeOf(reply.body) as { details?: { reason: string; pointer: string } } | null;
	const details = (reply.body as { error?: { details?: { reason: string; pointer: string } } } | null)?.error?.details;
	const expected = fixture.reject;
	if (reply.status !== 422 || envelope === null || details === undefined || expected === undefined) {
		return { pass: false, detail: `expected 422 with details, got ${reply.status}`, reply };
	}
	const pass = details.reason === expected.code && details.pointer === expected.path;
	return {
		pass,
		detail: pass
			? `refused: ${details.reason} at ${details.pointer || '(root)'}`
			: `expected ${expected.code} at ${expected.path}, got ${details.reason} at ${details.pointer}`,
		reply
	};
}
