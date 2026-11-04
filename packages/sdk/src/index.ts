// `@colophon-cms/sdk` — the TypeScript client for Colophon CMS's API.
//
//   import { Client, newIdempotencyKey } from '@colophon-cms/sdk';
//   const client = new Client({ baseUrl: 'http://localhost:58000', token });
//   const env = client.environment(projectId);            // production by default
//   const { data: draft, response } = await env.getEntry('articles', entryId);
//   await env.saveEntry('articles', entryId, { ...draft.content, title: 'New' },
//     { ifMatch: response.etag!, idempotencyKey: newIdempotencyKey() });
//
// The client carries a credential, it never creates one: signing in is `/auth/…`, `colophon
// login`, or the Studio — not this package's job (one API, no bypass). Every method's types come
// from `api.types.ts`, regenerated from the served OpenAPI document; failures arrive as
// `ApiError` with the error envelope's code, message, request id and details.

export { Client, Environment, Delivery, newIdempotencyKey } from './client.js';
export type {
	ClientOptions,
	SessionCredential,
	CallOptions,
	ReadOptions,
	WriteOptions,
	MatchOptions,
	GuardedOptions,
	Reply,
	ReplyMeta,
	RequestJson,
	ReplyJson,
	QueryOf
} from './client.js';
export { ApiError, errorOf } from './error.js';
export type { ErrorEnvelope, ErrorCode, ErrorDetails, ClientCode } from './error.js';

/* The whole contract, for anyone building on the wire shapes directly. */
export type { paths, operations, components } from './api.types.js';

/* The schemas a caller meets most, named as the document names them. */
import type { components } from './api.types.js';
export type Liveness = components['schemas']['Liveness'];
export type ReadinessReport = components['schemas']['ReadinessReport'];
export type VersionReport = components['schemas']['VersionReport'];
export type Me = components['schemas']['Me'];
export type ProjectView = components['schemas']['ProjectView'];
export type PermissionView = components['schemas']['PermissionView'];
export type EntryDocument = components['schemas']['EntryDocument'];
export type DraftView = components['schemas']['DraftView'];
export type EntrySummaryView = components['schemas']['EntrySummaryView'];
export type RevisionView = components['schemas']['RevisionView'];
export type RevisionContentView = components['schemas']['RevisionContentView'];
export type DeletedView = components['schemas']['DeletedView'];
export type QueryDocument = components['schemas']['QueryDocument'];
export type QueryPage = components['schemas']['QueryPage'];
export type QueriedView = components['schemas']['QueriedView'];
export type AppliedQuery = components['schemas']['AppliedQuery'];
export type TypeDefinition = components['schemas']['TypeDefinition'];
export type TypeVersionView = components['schemas']['TypeVersionView'];
export type NewType = components['schemas']['NewType'];
export type ChangesetView = components['schemas']['ChangesetView'];
export type ChangesetSummaryView = components['schemas']['ChangesetSummaryView'];
export type NewChangeset = components['schemas']['NewChangeset'];
export type NewReview = components['schemas']['NewReview'];
export type NewRelease = components['schemas']['NewRelease'];
export type PublishedView = components['schemas']['PublishedView'];
export type ReleaseView = components['schemas']['ReleaseView'];
export type ReceiptView = components['schemas']['ReceiptView'];
export type CommentView = components['schemas']['CommentView'];
export type NewComment = components['schemas']['NewComment'];
export type TaskView = components['schemas']['TaskView'];
export type NewTask = components['schemas']['NewTask'];
export type TaskUpdate = components['schemas']['TaskUpdate'];
export type MigrationSummaryView = components['schemas']['MigrationSummaryView'];
export type PlanView = components['schemas']['PlanView'];
export type MigrationItemView = components['schemas']['MigrationItemView'];
export type AssetView = components['schemas']['AssetView'];
export type RichTextDocument = components['schemas']['RichTextDocument'];
export type RichTextVerdict = components['schemas']['RichTextVerdict'];
export type PublishedEntryView = components['schemas']['PublishedEntryView'];
export type DeliveryQueryPage = components['schemas']['DeliveryQueryPage'];
