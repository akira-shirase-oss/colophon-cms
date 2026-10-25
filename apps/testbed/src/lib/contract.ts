// The HTTP contract of step 0.3, checked from a browser exactly as a client sees it. The same
// list runs on the Contract page and in Playwright (tests/contract.spec.ts). Each check says
// what it proves; a check that can't fail proves nothing, so each one asserts specifics.

import { call, envelopeOf, isReadinessReport, isUuidV7, type Reply } from './api.ts';

export interface Outcome {
	pass: boolean;
	detail: string;
	reply: Reply | null;
}

export interface Check {
	id: string;
	title: string;
	proves: string;
	run: () => Promise<Outcome>;
}

function outcome(reply: Reply, problems: string[]): Outcome {
	return {
		pass: problems.length === 0,
		detail: problems.length === 0 ? `HTTP ${reply.status} in ${reply.ms.toFixed(1)} ms` : problems.join('; '),
		reply
	};
}

/** Problems with an error envelope: wrong status, wrong code, or a request id that doesn't match. */
function envelopeProblems(reply: Reply, status: number, code: string): string[] {
	const problems: string[] = [];
	if (reply.status !== status) problems.push(`expected HTTP ${status}, got ${reply.status}`);
	const envelope = envelopeOf(reply.body);
	if (envelope === null) {
		problems.push('no error envelope in the body');
		return problems;
	}
	if (envelope.code !== code) problems.push(`expected code ${code}, got ${envelope.code}`);
	if (envelope.request_id === null || envelope.request_id !== reply.requestId) {
		problems.push(`envelope request_id ${envelope.request_id} differs from the header ${reply.requestId}`);
	}
	if (!(reply.headers.get('content-type') ?? '').startsWith('application/json')) {
		problems.push('the envelope is not served as application/json');
	}
	return problems;
}

function randomSuffix(): string {
	return Math.random().toString(36).slice(2, 10);
}

export const checks: Check[] = [
	{
		id: 'livez',
		title: 'GET /livez answers live',
		proves: 'Liveness answers without consulting any dependency, uncached, with a fresh UUIDv7 request id.',
		async run() {
			const reply = await call('GET', '/livez');
			const problems: string[] = [];
			if (reply.status !== 200) problems.push(`expected 200, got ${reply.status}`);
			if (reply.text !== '{"status":"live"}') problems.push(`unexpected body ${reply.text}`);
			if (reply.headers.get('cache-control') !== 'no-store') problems.push('missing Cache-Control: no-store');
			if (!isUuidV7(reply.requestId)) problems.push(`request id ${reply.requestId} is not a UUIDv7`);
			return outcome(reply, problems);
		}
	},
	{
		id: 'readyz',
		title: 'GET /readyz reports a consistent state',
		proves: 'The report has the documented shape, and the HTTP status agrees with it: 200 only for ready or degraded.',
		async run() {
			const reply = await call('GET', '/readyz');
			const problems: string[] = [];
			if (!isReadinessReport(reply.body)) {
				problems.push('the body is not a readiness report');
				return outcome(reply, problems);
			}
			const accepts = reply.body.status === 'ready' || reply.body.status === 'degraded';
			if (reply.status !== (accepts ? 200 : 503)) {
				problems.push(`status ${reply.body.status} came with HTTP ${reply.status}`);
			}
			for (const name of ['database', 'cache']) {
				if (!(name in reply.body.checks)) problems.push(`no ${name} check`);
			}
			return outcome(reply, problems);
		}
	},
	{
		id: 'head',
		title: 'HEAD /livez works',
		proves: 'Health checkers that only send HEAD get the same answer.',
		async run() {
			const reply = await call('HEAD', '/livez');
			return outcome(reply, reply.status === 200 ? [] : [`expected 200, got ${reply.status}`]);
		}
	},
	{
		id: 'not-found',
		title: 'An unknown route gets the envelope',
		proves: 'The fallback answers 404 route_not_found, and the envelope repeats the response header’s request id.',
		async run() {
			const reply = await call('GET', '/api/v1/testbed-does-not-exist');
			return outcome(reply, envelopeProblems(reply, 404, 'route_not_found'));
		}
	},
	{
		id: 'method',
		title: 'A wrong method gets the envelope',
		proves: 'axum’s own 405 is rewritten into the envelope, and its Allow header survives.',
		async run() {
			const reply = await call('DELETE', '/livez');
			const problems = envelopeProblems(reply, 405, 'method_not_allowed');
			if (!(reply.headers.get('allow') ?? '').includes('GET')) problems.push('the Allow header is missing or lacks GET');
			return outcome(reply, problems);
		}
	},
	{
		id: 'too-large',
		title: 'A 2 MiB body is refused with the envelope',
		proves: 'The 1 MiB body limit holds, and tower-http’s plain-text 413 is rewritten into the envelope.',
		async run() {
			const reply = await call('POST', '/api/v1/testbed-upload', { body: new Uint8Array(2 * 1024 * 1024) });
			return outcome(reply, envelopeProblems(reply, 413, 'payload_too_large'));
		}
	},
	{
		id: 'client-id',
		title: 'A client’s request id is kept',
		proves: 'A well-formed x-request-id from the client comes back in the header and in the envelope, so one id follows a request everywhere.',
		async run() {
			const id = `testbed-${randomSuffix()}`;
			const reply = await call('GET', '/api/v1/testbed-nope', { headers: { 'x-request-id': id } });
			const problems = envelopeProblems(reply, 404, 'route_not_found');
			if (reply.requestId !== id) problems.push(`sent ${id}, got ${reply.requestId}`);
			return outcome(reply, problems);
		}
	},
	{
		id: 'hostile-id',
		title: 'A hostile request id is replaced',
		proves: 'An over-long id is discarded and replaced with a fresh UUIDv7, so it never reaches the logs.',
		async run() {
			const hostile = 'a'.repeat(65);
			const reply = await call('GET', '/livez', { headers: { 'x-request-id': hostile } });
			const problems: string[] = [];
			if (reply.requestId === hostile) problems.push('the hostile id was echoed back');
			if (!isUuidV7(reply.requestId)) problems.push(`the replacement ${reply.requestId} is not a UUIDv7`);
			return outcome(reply, problems);
		}
	}
];
