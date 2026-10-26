// The Studio's only way to talk to colophon-server, which serves it on the same origin
// (D-037): the session cookie travels by itself, and writes echo the CSRF token.
//
// Every failure is a typed `ApiError` saying what kind it is, because the editor reacts to each
// differently: a conflict opens the conflict screen, a refusal shows the reason, and a network
// failure or a busy server means "keep the edit and try again".

export interface Project {
	id: string;
	slug: string;
	name: string;
	role: Role;
}

export interface Me {
	id: string;
	issuer: string;
	/** Their id at the identity provider: what `colophon admin bootstrap` binds to. */
	subject: string;
	display_name: string | null;
	/** Whether they may create projects (D-043). */
	administrator: boolean;
	projects: Project[];
}

/** One field of a content type, as the server writes definitions: every default spelled out. */
export type FieldDefinition = { id: string; label: string; required: boolean } & (
	| { type: 'text'; max_chars: number; multiline: boolean }
	| { type: 'rich_text' }
	| { type: 'number'; integer: boolean; min?: number; max?: number }
	| { type: 'boolean' }
	| { type: 'enum'; values: string[] }
	| { type: 'datetime' }
);

/** A content type's definition (D-040). */
export interface Definition {
	name: string;
	title_field: string;
	fields: FieldDefinition[];
}

/** One version of a content type. */
export interface ContentType {
	slug: string;
	version: number;
	definition: Definition;
	definition_hash: string;
	created_at: string;
}

/** An entry's content: field id to value, as its content type allows. */
export type Content = Record<string, unknown>;

export interface Revision {
	id: string;
	base_revision_id: string | null;
	author_id: string;
	created_at: string;
	content_hash: string;
	title: string;
}

export interface Draft {
	id: string;
	collection: string;
	created_at: string;
	updated_at: string;
	revision: Revision;
	content: Content;
}

export interface Summary {
	id: string;
	updated_at: string;
	revision: Revision;
}

/** A draft and the ETag that names its revision, for the next save's `If-Match`. */
export interface Loaded {
	draft: Draft;
	etag: string;
	/** True when the server answered from its idempotency record (a retried request). */
	replayed: boolean;
}

export type Failure =
	/** Not signed in, or the session ended. */
	| 'signed-out'
	/** The draft moved on since this revision; `current` names the new one. */
	| 'conflict'
	/** The server refused the content or the request; `message` says why. */
	| 'refused'
	/** No such project or article, or not one this person may see. */
	| 'not-found'
	/** No answer, or the server is busy or failing: the same request can be sent again. */
	| 'retry';

export class ApiError extends Error {
	constructor(
		readonly failure: Failure,
		message: string,
		readonly status: number | null = null,
		readonly reason: string | null = null,
		/** For a conflict: the current revision's ETag. */
		readonly current: string | null = null
	) {
		super(message);
	}
}

/** The session's CSRF token, which every cookie-authenticated write echoes. */
export function csrfToken(): string {
	const match = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='));
	return match?.slice('colophon_csrf='.length) ?? '';
}

/** Where "Sign in" goes; the server comes back to `returnTo` afterwards. */
export function signInUrl(returnTo: string): string {
	return `/auth/login?return_to=${encodeURIComponent(returnTo)}`;
}

/** Every request gives up after this long and counts as "no answer": a save is then retried with
 *  the same key, so nothing waits forever (invariant ST4). */
export const REQUEST_TIMEOUT_MS = 15_000;

interface Envelope {
	error?: { code?: string; message?: string; details?: { reason?: string } };
}

async function send(method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<Response> {
	const write = method !== 'GET';
	let response: Response;
	try {
		response = await fetch(path, {
			method,
			cache: 'no-store',
			signal: AbortSignal.timeout(REQUEST_TIMEOUT_MS),
			headers: {
				Accept: 'application/json',
				...(write ? { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() } : {}),
				...headers
			},
			...(body === undefined ? {} : { body: JSON.stringify(body) })
		});
	} catch {
		throw new ApiError('retry', 'no answer from the server');
	}
	if (response.ok) return response;
	const envelope = ((await response.json().catch(() => ({}))) as Envelope).error;
	const message = envelope?.message ?? `HTTP ${response.status}`;
	const reason = envelope?.details?.reason ?? null;
	switch (true) {
		case response.status === 401:
			throw new ApiError('signed-out', message, 401);
		case response.status === 404:
			throw new ApiError('not-found', message, 404);
		case response.status === 409 && reason === 'stale_revision':
			throw new ApiError('conflict', message, 409, reason, response.headers.get('etag'));
		case response.status >= 500 || response.status === 429:
			throw new ApiError('retry', message, response.status, reason);
		default:
			throw new ApiError('refused', message, response.status, reason);
	}
}

async function loaded(response: Response): Promise<Loaded> {
	const { data } = (await response.json()) as { data: Draft };
	return {
		draft: data,
		etag: response.headers.get('etag') ?? '',
		replayed: response.headers.get('idempotent-replayed') === 'true'
	};
}

const base = (project: string) => `/api/v1/projects/${project}/environments/production`;
const entries = (project: string, collection: string) => `${base(project)}/collections/${collection}/entries`;

/** Who is signed in, or `null` if nobody is. */
export async function me(): Promise<Me | null> {
	try {
		return ((await (await send('GET', '/me')).json()) as { data: Me }).data;
	} catch (error) {
		if (error instanceof ApiError && error.failure === 'signed-out') return null;
		throw error;
	}
}

/** Administrators only (D-043); the creator becomes the project's owner. */
/**
 * One project the signed-in person belongs to, with their role in it. Pages read the role from
 * here, not from `me().projects`, which lists at most 1,000 projects.
 */
export async function getProject(project: string): Promise<Project> {
	return ((await (await send('GET', `/api/v1/projects/${project}`)).json()) as { data: Project }).data;
}

export async function createProject(slug: string, name: string): Promise<Project> {
	return ((await (await send('POST', '/api/v1/projects', { slug, name })).json()) as { data: Project }).data;
}

export async function listContentTypes(project: string): Promise<ContentType[]> {
	return ((await (await send('GET', `${base(project)}/content-types`)).json()) as { data: ContentType[] }).data;
}

export async function getContentType(project: string, collection: string): Promise<ContentType> {
	return ((await (await send('GET', `${base(project)}/content-types/${collection}`)).json()) as { data: ContentType }).data;
}

export async function listEntries(project: string, collection: string): Promise<Summary[]> {
	return ((await (await send('GET', `${entries(project, collection)}?limit=100`)).json()) as { data: Summary[] }).data;
}

export async function getEntry(project: string, collection: string, entry: string): Promise<Loaded> {
	return loaded(await send('GET', `${entries(project, collection)}/${entry}`));
}

export async function createEntry(project: string, collection: string, content: Content, key: string): Promise<Loaded> {
	return loaded(await send('POST', entries(project, collection), content, { 'Idempotency-Key': key }));
}

/** Saves `content` on top of the revision `etag` names. Sending the same key again is safe. */
export async function saveEntry(
	project: string,
	collection: string,
	entry: string,
	content: Content,
	etag: string,
	key: string
): Promise<Loaded> {
	return loaded(
		await send('PUT', `${entries(project, collection)}/${entry}`, content, { 'If-Match': etag, 'Idempotency-Key': key })
	);
}

export async function listRevisions(project: string, collection: string, entry: string): Promise<Revision[]> {
	return ((await (await send('GET', `${entries(project, collection)}/${entry}/revisions`)).json()) as { data: Revision[] }).data;
}

/** A terminal asking to sign in (D-041), as the approval page shows it. */
export interface DeviceLogin {
	client_name: string;
	expires_at: string;
	approved: boolean;
}

export async function deviceLogin(code: string): Promise<DeviceLogin> {
	return ((await (await send('GET', `/auth/device/${encodeURIComponent(code)}`)).json()) as { data: DeviceLogin }).data;
}

/** Lets the terminal showing `code` sign in as the current person. */
export async function approveDevice(code: string): Promise<void> {
	await send('POST', '/auth/device/approve', { user_code: code });
}

/** What a member may do in a project (D-042): owners everything, developers all but members,
 *  editors and publishers content. */
export type Role = 'owner' | 'developer' | 'editor' | 'publisher';
export const ROLES: readonly Role[] = ['owner', 'developer', 'editor', 'publisher'];

/** What each role may do, in a few words, for the members and invitation pages. */
export const ROLE_SUMMARY: Record<Role, string> = {
	owner: 'everything, including who belongs',
	developer: 'content and content types',
	editor: 'content',
	publisher: 'content (publishing comes later)'
};

export interface Member {
	identity_id: string;
	display_name: string | null;
	role: Role;
	joined_at: string;
}

/** A pending invitation; `link` only in the answer that made it, because it carries the token. */
export interface Invitation {
	id: string;
	role: Role;
	label: string;
	created_at: string;
	expires_at: string;
	link?: string;
}

/** What an invitation offers, as the person holding its link sees it. */
export interface Offer {
	project_id: string;
	project_name: string;
	role: Role;
	invited_by: string | null;
	expires_at: string;
}

const members = (project: string) => `/api/v1/projects/${project}/members`;
const invitations = (project: string) => `/api/v1/projects/${project}/invitations`;

export async function listMembers(project: string): Promise<Member[]> {
	return ((await (await send('GET', members(project))).json()) as { data: Member[] }).data;
}

/** Owners only; refused when it would leave the project without an owner. */
export async function changeRole(project: string, identity: string, role: Role): Promise<void> {
	await send('PUT', `${members(project)}/${identity}`, { role });
}

/** An owner removing someone, or anyone removing themselves (leaving). */
export async function removeMember(project: string, identity: string): Promise<void> {
	await send('DELETE', `${members(project)}/${identity}`);
}

export async function listInvitations(project: string): Promise<Invitation[]> {
	return ((await (await send('GET', invitations(project))).json()) as { data: Invitation[] }).data;
}

export async function createInvitation(project: string, role: Role, label: string): Promise<Invitation> {
	return ((await (await send('POST', invitations(project), { role, label })).json()) as { data: Invitation }).data;
}

export async function revokeInvitation(project: string, invitation: string): Promise<void> {
	await send('DELETE', `${invitations(project)}/${invitation}`);
}

/** The token travels in a body, never in a URL the server would see or log. */
export async function lookupInvitation(token: string): Promise<Offer> {
	return ((await (await send('POST', '/api/v1/invitations/lookup', { token })).json()) as { data: Offer }).data;
}

export async function acceptInvitation(token: string): Promise<{ project_id: string; role: Role }> {
	return ((await (await send('POST', '/api/v1/invitations/accept', { token })).json()) as { data: { project_id: string; role: Role } })
		.data;
}

/** Where a changeset stands (D-044); only `approved` can be published (D-045). */
export type ChangesetState = 'open' | 'stale' | 'awaiting_review' | 'changes_requested' | 'approved' | 'approval_expired' | 'published';

export interface ChangesetStatus {
	state: ChangesetState;
	/** For `published`: the release's generation. */
	generation: number | null;
	/** Who approved or asked for changes. */
	by: string | null;
	/** The approver also submitted it: allowed, and always shown. */
	self_review: boolean;
	/** Entries that moved on since they were pinned. */
	edited_items: number;
}

export interface ChangesetItem {
	/** Publish the pinned revision, or take it down (D-049). */
	kind: 'publish' | 'unpublish';
	entry_id: string;
	collection: string;
	revision_id: string;
	title: string;
	content_hash: string;
	schema_version: number;
	/** For `publish`, the entry's current revision; for `unpublish`, the published one (null once
	 *  nothing is). */
	current_revision_id: string | null;
	/** The entry moved on since it was pinned. */
	edited: boolean;
}

export interface ChangesetReview {
	id: string;
	reviewer_id: string;
	reviewer_name: string | null;
	decision: 'approve' | 'request_changes';
	changeset_hash: string;
	/** Made on the changeset's current hash, so it still counts. */
	current: boolean;
	comment: string;
	created_at: string;
}

export interface Changeset {
	id: string;
	title: string;
	hash: string;
	status: ChangesetStatus;
	created_by: string;
	created_at: string;
	submitted_by: string | null;
	submitted_at: string | null;
	updated_at: string;
	items: ChangesetItem[];
	reviews: ChangesetReview[];
}

export interface ChangesetSummary {
	id: string;
	title: string;
	hash: string;
	status: ChangesetStatus;
	items: number;
	/** How many of its items unpublish. */
	unpublish: number;
	submitted_at: string | null;
	updated_at: string;
}

const changesets = (project: string) => `${base(project)}/changesets`;

export async function listChangesets(project: string): Promise<ChangesetSummary[]> {
	return ((await (await send('GET', changesets(project))).json()) as { data: ChangesetSummary[] }).data;
}

export async function getChangeset(project: string, id: string): Promise<Changeset> {
	return ((await (await send('GET', `${changesets(project)}/${id}`)).json()) as { data: Changeset }).data;
}

/** Pins each entry's current revision to publish, and the published revision of each entry in
 *  `unpublish` to take down (D-049). Sending the same key again is safe. */
export async function createChangeset(
	project: string,
	title: string,
	entries: string[],
	unpublish: string[],
	key: string
): Promise<Changeset> {
	const body = { title, entries, unpublish };
	return ((await (await send('POST', changesets(project), body, { 'Idempotency-Key': key })).json()) as { data: Changeset }).data;
}

/** Submits, refreshes or reviews the changeset whose hash the caller saw; a changed hash is refused
 *  with reason `stale_changeset`. */
export async function changesetAction(
	project: string,
	id: string,
	hash: string,
	action: 'submit' | 'refresh' | 'reviews',
	body?: { decision: 'approve' | 'request_changes'; comment: string }
): Promise<Changeset> {
	const path = `${changesets(project)}/${id}/${action}`;
	return ((await (await send('POST', path, body, { 'If-Match': `"${hash}"` })).json()) as { data: Changeset }).data;
}

/** A committed release (D-045). */
export interface Release {
	generation: number;
	epoch: number;
	changeset_id: string;
	title: string;
	changeset_hash: string;
	published_by: string;
	published_by_name: string | null;
	published_at: string;
	/** Entries whose published revision changed in it. */
	entries: number;
	/** Entries it took down. */
	unpublished: number;
	/** Whether the environment's CDNs accepted a purge covering it (D-050); until then, copies they
	 *  hold may still be served. */
	purge: 'none' | 'pending' | 'accepted' | 'failed';
	/** Its one label (D-051): in the CMS only, update pending, verified at `locations` locations,
	 *  or needs attention. */
	label: 'published' | 'pending' | 'verified' | 'needs_attention';
	/** How many distinct locations a probe saw this release, or a later one, from. */
	locations: number;
}

/** One destination's part in delivering a release. */
export interface Delivery {
	destination_id: string;
	name: string;
	kind: Destination['kind'];
	location: string | null;
	/** `covered` means a later release's delivery covered this one (see `covered_by`). */
	state: 'done' | 'covered' | 'pending' | 'failed';
	covered_by: number | null;
	status: Job['status'] | null;
	attempts: number | null;
	last_error: string | null;
	finished_at: string | null;
}

/** One look a probe took for a release. */
export interface Observation {
	destination_id: string;
	location: string;
	/** The release the answer said it was built from, if it said. */
	observed_generation: number | null;
	http_status: number | null;
	body_hash: string | null;
	error: string | null;
	observed_at: string;
}

/** A release's receipt: its label, each destination's part and every probe look, oldest first. */
export interface Receipt extends Release {
	deliveries: Delivery[];
	observations: Observation[];
}

export async function getReceipt(project: string, generation: number): Promise<Receipt> {
	return ((await (await send('GET', `${base(project)}/releases/${generation}`)).json()) as { data: Receipt }).data;
}

/** The environment's releases, newest first, and its current generation. */
export async function listReleases(project: string): Promise<{ releases: Release[]; generation: number }> {
	const response = await send('GET', `${base(project)}/releases`);
	const generation = Number(response.headers.get('colophon-generation') ?? '0');
	return { releases: ((await response.json()) as { data: Release[] }).data, generation };
}

/** Publishes the approved changeset with `hash` as the release after `baseGeneration`; refused
 *  with reason `generation_moved` if another release committed first. Sending the same key again
 *  is safe. */
export async function publishChangeset(
	project: string,
	id: string,
	hash: string,
	baseGeneration: number,
	key: string
): Promise<{ generation: number; entries_changed: number; entries_unpublished: number }> {
	const path = `${changesets(project)}/${id}/publish`;
	const response = await send('POST', path, { base_generation: baseGeneration }, { 'If-Match': `"${hash}"`, 'Idempotency-Key': key });
	return ((await response.json()) as { data: { generation: number; entries_changed: number; entries_unpublished: number } }).data;
}

/** Where a project's releases are announced (D-046). */
export interface Destination {
	id: string;
	/** A webhook hears each release; a CDN is told what to purge (D-050); a probe looks for it
	 *  (D-051). */
	kind: 'webhook' | 'cdn' | 'probe';
	name: string;
	url: string;
	enabled: boolean;
	/** For a probe: the only location whose workers run it, if pinned. */
	location: string | null;
	created_at: string;
}

/** One release's announcement to one destination. */
export interface Job {
	id: string;
	destination_id: string;
	destination_name: string;
	generation: number;
	status: 'pending' | 'running' | 'retrying' | 'succeeded' | 'failed' | 'superseded';
	attempts: number;
	last_http_status: number | null;
	last_error: string | null;
	created_at: string;
	finished_at: string | null;
}

/** Owners and developers only: an address may carry a secret. */
export async function listDestinations(project: string): Promise<Destination[]> {
	return ((await (await send('GET', `${base(project)}/destinations`)).json()) as { data: Destination[] }).data;
}

export async function createDestination(
	project: string,
	kind: Destination['kind'],
	name: string,
	url: string,
	location: string | null = null
): Promise<Destination> {
	const body = { kind, name, url, location };
	return ((await (await send('POST', `${base(project)}/destinations`, body)).json()) as { data: Destination }).data;
}

export async function setDestinationEnabled(project: string, id: string, enabled: boolean): Promise<void> {
	await send('PUT', `${base(project)}/destinations/${id}`, { enabled });
}

export async function listJobs(project: string): Promise<Job[]> {
	return ((await (await send('GET', `${base(project)}/jobs`)).json()) as { data: Job[] }).data;
}

/** Where anyone can read what the project publishes, with no sign-in. */
export function deliveryUrl(project: string, collection: string): string {
	return `/content/v1/${project}/production/${collection}`;
}

/** A fresh idempotency key: 32 hex characters. */
export function newKey(): string {
	return crypto.randomUUID().replaceAll('-', '');
}
