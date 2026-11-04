// The one error envelope every failure answers in, parsed once into `ApiError` (the same shape
// the Studio's api.ts and the CLI build by hand). `code` is stable and machine-readable — branch
// on it (and on `details.reason`), never on `message`.

import type { components } from './api.types.js';

/** The error envelope, as the served OpenAPI document declares it. */
export type ErrorEnvelope = components['schemas']['Error'];
/** The envelope's stable codes. The semantic *reason* (`stale_revision`, `missing_permission`,
 *  `slug_taken`) lives one level down, in `details.reason`. */
export type ErrorCode = ErrorEnvelope['error']['code'];
/** `error.details`: a `reason`, a JSON `pointer` into the request body, and whatever the refusal
 *  lists (entries in the way, the missing permission, candidates for an ambiguous time). */
export type ErrorDetails = NonNullable<ErrorEnvelope['error']['details']>;

/** Codes the SDK raises itself — never a server's envelope code. */
export type ClientCode =
	/** Nothing came back: no route, refused connection, or the caller's/this client's deadline. */
	| 'no_answer'
	/** The reply was not the JSON the contract promises (a proxy page, a truncated body). */
	| 'invalid_response'
	/** The body ran past `ClientOptions.maxBodyBytes`; nothing was parsed. */
	| 'response_too_large';

/**
 * Every failure a call can meet, as one typed error: the HTTP `status` (0 when no answer ever
 * arrived), the envelope `code` and `details`, the `requestId` to quote in support, and, for a
 * conflict, the `etag` the resource holds now — the `ifMatch` a retry would send.
 */
export class ApiError extends Error {
	override readonly name = 'ApiError';
	/** The envelope's `request_id`, or the `x-request-id` header's; `null` when neither came. */
	readonly requestId: string | null;
	/** `error.details` — `reason`, `pointer`, `entries`, `permission`, `candidates`; `null` when
	 *  the server gave none. */
	readonly details: ErrorDetails | null;
	/** The `ETag` the failed response carried — for a 409 conflict, the current revision's. */
	readonly etag: string | null;
	/** `Retry-After` as milliseconds, when a 429 named a wait. */
	readonly retryAfterMs: number | null;

	constructor(
		/** The HTTP status, or 0 when no response ever arrived. */
		readonly status: number,
		/** The envelope's `error.code`, or a `ClientCode` the SDK raised itself. */
		readonly code: ErrorCode | ClientCode,
		message: string,
		init: {
			requestId?: string | null;
			details?: ErrorDetails | null;
			etag?: string | null;
			retryAfterMs?: number | null;
			cause?: unknown;
		} = {}
	) {
		super(message, { cause: init.cause });
		this.requestId = init.requestId ?? null;
		this.details = init.details ?? null;
		this.etag = init.etag ?? null;
		this.retryAfterMs = init.retryAfterMs ?? null;
	}

	/** The refusal's semantic reason (`details.reason`), or `null`. */
	get reason(): string | null {
		return this.details?.reason ?? null;
	}

	/** The JSON Pointer into the request body the refusal names (`details.pointer`), or `null`. */
	get pointer(): string | null {
		return this.details?.pointer ?? null;
	}

	/**
	 * Whether the same request is allowed to go again without a person looking first: nothing was
	 * written — no answer, a rate limit, a deadline or a 5xx — or nothing was decided (`429`). A
	 * retried write MUST carry the same `idempotencyKey`, so the server answers the stored result
	 * instead of writing twice (invariant K2). Conflicts, refusals and 4xxs are not retryable:
	 * they answer the same way every time.
	 */
	get retryable(): boolean {
		return (
			this.status === 0 ||
			this.status === 429 ||
			this.status >= 500 ||
			this.code === 'deadline_exceeded' ||
			this.code === 'rate_limited' ||
			this.code === 'unavailable'
		);
	}
}

/** Narrows a parsed body to the error envelope, or `null` when it isn't one. */
export function errorOf(body: unknown): ErrorEnvelope['error'] | null {
	if (typeof body !== 'object' || body === null || !('error' in body)) return null;
	const { error } = body as { error: unknown };
	if (typeof error !== 'object' || error === null) return null;
	const fields = error as Record<string, unknown>;
	if (typeof fields['code'] !== 'string' || typeof fields['message'] !== 'string') return null;
	return fields as unknown as ErrorEnvelope['error'];
}
