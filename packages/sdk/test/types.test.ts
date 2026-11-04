// Type-level coverage — `tsc --noEmit` is the test: the generated operation types must reach a
// caller as the exact wire shapes, the required write guards must be required, and the schema
// aliases must stay honest. `expectTypeOf` documents intent; `@ts-expect-error` proves refusal.
// (The stub fetch below exists so the calls vitest evaluates anyway resolve harmlessly.)

import { describe, expectTypeOf, it } from 'vitest';
import { Client } from '../src/index.ts';
import type {
	ApiError,
	DraftView,
	EntryDocument,
	EntrySummaryView,
	QueryDocument,
	QueryOf,
	Reply,
	ReplyJson,
	RequestJson,
	TypeDefinition
} from '../src/index.ts';

const ok = (async () => new Response('{}', { status: 200, headers: { 'content-type': 'application/json' } })) as typeof fetch;
const client = new Client({ baseUrl: 'http://server.test', fetch: ok });
const env = client.environment('p');

describe('generated operation types reach the caller', () => {
	it('request bodies are the document’s schemas', () => {
		expectTypeOf<RequestJson<'createEntry'>>().toEqualTypeOf<EntryDocument>();
		expectTypeOf<EntryDocument>().toEqualTypeOf<Record<string, unknown>>();
		expectTypeOf<RequestJson<'queryEntries'>>().toEqualTypeOf<QueryDocument>();
		expectTypeOf<RequestJson<'saveEntry'>>().toEqualTypeOf<EntryDocument>();
	});

	it('reply bodies are the wire shapes, names kept', () => {
		expectTypeOf<ReplyJson<'listEntries'>>().toMatchTypeOf<{ data: EntrySummaryView[]; next_cursor: string | null }>();
		expectTypeOf<ReplyJson<'getEntry'>>().toMatchTypeOf<{ data: DraftView }>();
		expectTypeOf<ReplyJson<'serverVersion'>>().toMatchTypeOf<{ version: string; api: string[] }>();
	});

	it('a Reply adds response metadata without hiding the body', () => {
		type R = Reply<{ data: DraftView }>;
		expectTypeOf<R['data']>().toEqualTypeOf<DraftView>();
		expectTypeOf<R['response']['etag']>().toEqualTypeOf<string | null>();
		expectTypeOf<R['response']['status']>().toEqualTypeOf<number>();
		expectTypeOf<R['response']['replayed']>().toEqualTypeOf<boolean>();
	});

	it('query params come from the document, not a hand copy', () => {
		expectTypeOf<QueryOf<'listEntries'>>().toMatchTypeOf<{
			limit?: number;
			cursor?: string;
			q?: string;
			created_by?: string;
			locale?: string;
		}>();
		expectTypeOf<QueryOf<'listTasks'>>().toMatchTypeOf<{ status?: string; collection?: string; entry?: string }>();
	});
});

describe('the guards the routes require are required in types', () => {
	it('a write needs its idempotency key', () => {
		// @ts-expect-error — idempotencyKey is required
		void env.createEntry('articles', {}, {});
		// @ts-expect-error — idempotencyKey is required
		void env.createComment('articles', 'e1', { revision: 'r', body: 'hi' }, {});
	});

	it('a guarded write needs If-Match too', () => {
		// @ts-expect-error — ifMatch is required
		void env.saveEntry('articles', 'e1', {}, { idempotencyKey: 'k-abcdefghijklmnop' });
		// @ts-expect-error — idempotencyKey is required
		void env.saveEntry('articles', 'e1', {}, { ifMatch: '"r"' });
		// @ts-expect-error — ifMatch is required
		void env.reviewChangeset('c1', { decision: 'approve' }, {});
		// @ts-expect-error — ifMatch is required
		void env.submitChangeset('c1', {});
	});

	it('reads take no key and no match', () => {
		expectTypeOf(env.getEntry).parameter(2).toEqualTypeOf<{ signal?: AbortSignal } | undefined>();
	});
});

describe('method returns stay typed', () => {
	it('methods resolve to their operation’s body', () => {
		expectTypeOf(env.getEntry).returns.toMatchTypeOf<Promise<{ data: DraftView }>>();
		expectTypeOf(client.version()).resolves.toMatchTypeOf<ReplyJson<'serverVersion'>>();
		expectTypeOf(env.listReleases()).resolves.toMatchTypeOf<ReplyJson<'listReleases'>>();
		expectTypeOf(env.getReceipt(4)).resolves.toMatchTypeOf<ReplyJson<'getReceipt'>>();
		void env.updateContentType('articles', {} satisfies TypeDefinition, { ifMatch: '"v1"', idempotencyKey: 'k' });
	});

	it('ApiError carries the typed envelope', () => {
		expectTypeOf<ApiError['retryable']>().toEqualTypeOf<boolean>();
		expectTypeOf<ApiError['pointer']>().toEqualTypeOf<string | null>();
		expectTypeOf<'unauthorized' | 'not_found' | 'no_answer'>().toMatchTypeOf<ApiError['code']>();
	});
});
