// The one way the testbed talks to colophon-server: through the same-origin `/server` proxy
// (vite.config.ts), with no caching, recording everything a check might need.

export interface Reply {
	status: number;
	headers: Headers;
	/** The body parsed as JSON, or the raw text if it isn't JSON. */
	body: unknown;
	text: string;
	requestId: string | null;
	ms: number;
}

export interface CallOptions {
	headers?: Record<string, string>;
	body?: BodyInit;
}

export async function call(method: string, path: string, options: CallOptions = {}): Promise<Reply> {
	const started = performance.now();
	const response = await fetch(`/server${path}`, {
		method,
		headers: options.headers,
		body: options.body,
		cache: 'no-store'
	});
	const text = await response.text();
	let body: unknown = text;
	try {
		body = text === '' ? null : JSON.parse(text);
	} catch {
		// Not JSON: keep the text.
	}
	return {
		status: response.status,
		headers: response.headers,
		body,
		text,
		requestId: response.headers.get('x-request-id'),
		ms: performance.now() - started
	};
}

const UUID_V7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;

export function isUuidV7(value: string | null): boolean {
	return value !== null && UUID_V7.test(value);
}

/** The `/readyz` report, as the server documents it. */
export interface ReadinessReport {
	status: 'starting' | 'ready' | 'degraded' | 'not_ready' | 'draining';
	role: string;
	checks: Record<string, string>;
}

export function isReadinessReport(body: unknown): body is ReadinessReport {
	if (typeof body !== 'object' || body === null) return false;
	const report = body as Record<string, unknown>;
	return (
		typeof report['status'] === 'string' &&
		typeof report['role'] === 'string' &&
		typeof report['checks'] === 'object' &&
		report['checks'] !== null
	);
}

/** The error envelope every 4xx and 5xx carries. */
export interface Envelope {
	error: { code: string; message: string; request_id: string | null };
}

export function envelopeOf(body: unknown): Envelope['error'] | null {
	if (typeof body !== 'object' || body === null || !('error' in body)) return null;
	const error = (body as { error: unknown }).error;
	if (typeof error !== 'object' || error === null) return null;
	const fields = error as Record<string, unknown>;
	if (typeof fields['code'] !== 'string' || typeof fields['message'] !== 'string') return null;
	const requestId = fields['request_id'];
	return {
		code: fields['code'],
		message: fields['message'],
		request_id: typeof requestId === 'string' ? requestId : null
	};
}
