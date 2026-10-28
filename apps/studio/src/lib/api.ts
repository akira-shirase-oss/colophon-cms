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
	/** The caller's role, by key. */
	role: Role;
	/** That role's name, for people. */
	role_name: string;
	/** What that role allows; only when one project is read (D-058). */
	permissions?: Grant[];
	/** Whether whoever sends a publish request may approve it themselves (D-044), so one click
	 *  can publish (D-059); only when one project is read. */
	self_review?: boolean;
}

/** Something a member may try to do (D-058). */
export type Action =
	| 'content.read'
	| 'content.create'
	| 'content.update'
	| 'content.delete'
	| 'content.review'
	| 'content.publish'
	| 'schema.change'
	| 'members.manage'
	| 'roles.manage'
	| 'destinations.manage'
	| 'tokens.manage'
	| 'audit.read';

/** Actions only people hold: no API token may (step R.4), so a leaked token can't change who has
 *  access or read the record of it. */
export const PEOPLE_ONLY: Action[] = ['members.manage', 'roles.manage', 'tokens.manage', 'audit.read'];

/** One permission: an action, on `*` or one collection. */
export interface Grant {
	action: Action;
	subject: string;
	/** Only these fields (step R.3). */
	fields?: string[];
	/** `own`: only entries the member created (step R.3). */
	condition?: 'own';
}

/**
 * Whether the caller's role allows `action`, in `collection` when it's about one; `undefined`
 * asks for it everywhere. The same rule as the server's `Policy::allows`, so the Studio only
 * offers what will work; the server still decides every request.
 */
export function may(project: Pick<Project, 'permissions'> | undefined, action: Action, collection?: string): boolean {
	return (project?.permissions ?? []).some(
		(grant) => grant.action === action && (grant.subject === '*' || (collection !== undefined && grant.subject === collection))
	);
}

/**
 * Whether the role allows `action` on one entry of `collection`, `own` when the caller created it:
 * a permission limited to their own entries (step R.3) counts only then. `may` answers "somewhere";
 * this answers "on this entry".
 */
export function mayOn(project: Pick<Project, 'permissions'> | undefined, action: Action, collection: string, own: boolean): boolean {
	return (project?.permissions ?? []).some(
		(grant) =>
			grant.action === action &&
			(grant.subject === '*' || grant.subject === collection) &&
			(grant.condition === undefined || (grant.condition === 'own' && own))
	);
}

/** Whether the role holds any of `actions`, anywhere: for showing an area whose parts each ask
 *  for their own permission (Settings). */
export function mayAny(project: Pick<Project, 'permissions'> | undefined, actions: Action[]): boolean {
	return (project?.permissions ?? []).some((grant) => actions.includes(grant.action));
}

/** What Settings holds (D-059, rule 5): content types, website connections, roles and tokens. A
 *  role with none of these doesn't see Settings at all. */
export const SETTINGS_ACTIONS: Action[] = ['schema.change', 'destinations.manage', 'roles.manage', 'tokens.manage', 'audit.read'];

export interface Me {
	id: string;
	issuer: string;
	/** Their id at the identity provider: what `colophon admin bootstrap` binds to. */
	subject: string;
	display_name: string | null;
	/** Whether they may create projects (D-043). */
	administrator: boolean;
	/** The installation as a whole: until it has an administrator, the setup code the server
	 *  printed when it started makes the first one (D-061). */
	installation: { has_administrator: boolean };
	projects: Project[];
}

/** One field of a content type, as the server writes definitions: every default spelled out,
 *  except `description` and `min_chars`, which are written only when set (D-064). */
export type FieldDefinition = {
	id: string;
	label: string;
	required: boolean;
	/** Help text shown under the field, at most 500 characters. */
	description?: string;
	/** No two entries may share its value (D-070): top-level single-line text, email, URL, number,
	 *  date, time and date-time fields only. Written only when true. */
	unique?: boolean;
} & (
	/** `pattern` (D-069): the form every value must take, a regular expression matched against the
	 *  whole value on the server (Rust syntax; the Studio never imitates it). */
	| { type: 'text'; max_chars: number; multiline: boolean; min_chars?: number; pattern?: string }
	| { type: 'rich_text' }
	| { type: 'number'; integer: boolean; min?: number; max?: number }
	| { type: 'boolean' }
	| { type: 'enum'; values: string[] }
	| { type: 'datetime' }
	/** The entry's URL name, unique in its collection (D-054); `source` is the text field the
	 *  Studio suggests it from. */
	| { type: 'slug'; source?: string }
	/** Step M.1 (D-064): values in one fixed format each, written canonically by the server. */
	| { type: 'email' }
	/** Stored as `new URL(text).href`: http or https, with a host and no user name. */
	| { type: 'url' }
	/** `YYYY-MM-DD`, as typed. */
	| { type: 'date' }
	/** `HH:MM` or `HH:MM:SS`, as typed. */
	| { type: 'time' }
	/** Any JSON value, canonical (lib/json.ts). */
	| { type: 'json' }
	| { type: 'markdown'; max_chars: number }
	/** `#rrggbb`, lowercase. */
	| { type: 'color' }
	/** Step M.2 (D-065): entries of the collection `to`, by id: one, or a list of up to `max`
	 *  distinct ones. The database keeps them from dangling. */
	| { type: 'reference'; to: string; many: false }
	| { type: 'reference'; to: string; many: true; max: number }
	/** Step M.3 (D-066): structures, at most three deep, written in definition order. Nested
	 *  fields: a group of fields (Strapi's component). */
	| { type: 'object'; fields: FieldDefinition[] }
	/** Up to `max` values of one kind, never a list or page sections. */
	| { type: 'list'; of: ItemKind; max: number }
	/** A page builder: up to `max` sections, each one of its kinds, marked by `_block`. */
	| { type: 'blocks'; blocks: BlockKind[]; max: number }
);

/** What each item of a list is: a field's kind and options, without its id, label or flags. */
export type ItemKind = FieldDefinition extends infer Field
	? Field extends { type: 'list' | 'blocks' | 'slug' }
		? never
		: Field extends FieldDefinition
			? Omit<Field, 'id' | 'label' | 'required' | 'description'>
			: never
	: never;

/** One kind of page section: a name (what items' `_block` says), a label and its fields. */
export interface BlockKind {
	name: string;
	label: string;
	fields: FieldDefinition[];
}

/** How deep structures nest (D-066): a group, a list and page sections each add one level. */
export const MAX_DEPTH = 3;

/** The field types a content type can use, in the order the builder offers them. */
export const FIELD_TYPES = [
	'text',
	'rich_text',
	'markdown',
	'number',
	'boolean',
	'enum',
	'datetime',
	'date',
	'time',
	'email',
	'url',
	'color',
	'json',
	'reference',
	'object',
	'list',
	'blocks',
	'slug'
] as const;
export type FieldType = (typeof FIELD_TYPES)[number];

/** A content type's definition (D-040). */
export interface Definition {
	name: string;
	title_field: string;
	fields: FieldDefinition[];
	/** A single type (D-067): it holds one entry, like site settings or a home page. Written only
	 *  when true. */
	single?: boolean;
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
	/** Who created it: the member whose own entry it is. */
	created_by: string;
	updated_at: string;
	revision: Revision;
	/** Only the fields the person's role may read (D-058, step R.3). */
	content: Content;
	/** Fields left out because the role can't read them. */
	hidden_fields?: string[];
	/** Fields the role can read but not change. */
	locked_fields?: string[];
	/** What readers get now, on a read: the live revision and its release, or null when not
	 *  published. Create and save answers leave it out. */
	published?: { revision_id: string; generation: number } | null;
	/** On a read: entries that link to this one (at most 100, only in collections the role reads),
	 *  by field, and whether their draft and what readers get of them link to it (D-065). */
	referenced_by?: Referrer[];
}

/** An entry that links to another (D-065). */
export interface Referrer {
	entry_id: string;
	collection: string;
	field: string;
	/** Its draft links to it. */
	in_draft: boolean;
	/** What readers get of it links to it: taking the linked entry down is refused until that changes. */
	published: boolean;
}

export interface Summary {
	id: string;
	/** Who created it. */
	created_by: string;
	updated_at: string;
	revision: Revision;
	/** What readers get now, if published, and the release it went out in. */
	published: { revision_id: string; generation: number } | null;
}

/** Where an entry stands for readers: never published (or taken down), live as drafted, or
 *  changed since it went live. */
export type EntryState = 'draft' | 'live' | 'changed';

export function entryState(summary: Pick<Summary, 'revision' | 'published'>): EntryState {
	if (summary.published === null) return 'draft';
	return summary.published.revision_id === summary.revision.id ? 'live' : 'changed';
}

/** One page of a list, and the cursor for the next, if any. */
export interface Page<T> {
	data: T[];
	next_cursor: string | null;
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
		readonly current: string | null = null,
		/** What the server said about where and why, including entries in the way. */
		readonly details: Details | null = null
	) {
		super(message);
	}
}

/** A stored entry in the way of a request (D-054, D-055). */
export interface EntryInTheWay {
	entry: string;
	title: string;
	state: 'draft' | 'published';
	/** For a content type change: the refusal the entry's content would get. */
	problem?: { reason: string; pointer: string; message: string };
}

/** An error's `details`: a reason, a JSON Pointer into the request body, and for refusals that
 *  depend on stored content, the entries in the way (at most 20) and how many there are. */
export interface Details {
	reason?: string;
	pointer?: string;
	entries?: EntryInTheWay[];
	total?: number;
	/** For a local time the clocks show twice or skip: the two instants to choose from, earlier
	 *  first (D-060). */
	candidates?: string[];
	/** For a 403 `missing_permission`: the permission the role lacks, such as `content.publish`. */
	permission?: string;
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

/** Where "Create an account" goes: the identity provider's own sign-up form, then back here
 *  signed in. An account grants nothing by itself (D-043). */
export function signUpUrl(returnTo: string): string {
	return `/auth/login?start=create&return_to=${encodeURIComponent(returnTo)}`;
}

/** Every request gives up after this long and counts as "no answer": a save is then retried with
 *  the same key, so nothing waits forever (invariant ST4). */
export const REQUEST_TIMEOUT_MS = 15_000;

interface Envelope {
	error?: { code?: string; message?: string; details?: Details };
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
	const details = envelope?.details ?? null;
	const reason = details?.reason ?? null;
	switch (true) {
		case response.status === 401:
			throw new ApiError('signed-out', message, 401);
		case response.status === 404:
			throw new ApiError('not-found', message, 404);
		case response.status === 409 && (reason === 'stale_revision' || reason === 'stale_version' || reason === 'stale_role'):
			throw new ApiError('conflict', message, 409, reason, response.headers.get('etag'), details);
		case response.status >= 500 || response.status === 429:
			throw new ApiError('retry', message, response.status, reason);
		default:
			throw new ApiError('refused', message, response.status, reason, null, details);
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

/**
 * Ends this browser's session (`POST /auth/logout`, CSRF-checked) and answers where to end it at
 * the identity provider too, so the next person at this computer must sign in again; `null` when
 * the provider can't say. Already signed out counts as done: the outcome asked for holds either way.
 */
export async function signOut(): Promise<string | null> {
	try {
		const answer = (await (await send('POST', '/auth/logout')).json().catch(() => null)) as { data?: { provider_sign_out?: string | null } } | null;
		return answer?.data?.provider_sign_out ?? null;
	} catch (error) {
		if (error instanceof ApiError && error.failure === 'signed-out') return null;
		throw error;
	}
}

/**
 * One project the signed-in person belongs to, with their role in it. Pages read the role from
 * here, not from `me().projects`, which lists at most 1,000 projects.
 */
export async function getProject(project: string): Promise<Project> {
	return ((await (await send('GET', `/api/v1/projects/${project}`)).json()) as { data: Project }).data;
}

/** Makes the caller the installation's first administrator with the setup code its server printed
 *  (D-061). Refused with reason `setup_code_invalid`, `already_set_up`, or status 429 (too many
 *  attempts; the message says how long to wait). The code goes in the body, never a URL. */
export async function claimSetup(code: string): Promise<void> {
	await send('POST', '/api/v1/setup', { code });
}

/** Administrators only (D-043); the creator becomes the project's owner. */
export async function createProject(slug: string, name: string): Promise<Project> {
	return ((await (await send('POST', '/api/v1/projects', { slug, name })).json()) as { data: Project }).data;
}

export async function listContentTypes(project: string): Promise<ContentType[]> {
	return ((await (await send('GET', `${base(project)}/content-types`)).json()) as { data: ContentType[] }).data;
}

export async function getContentType(project: string, collection: string): Promise<ContentType> {
	return ((await (await send('GET', `${base(project)}/content-types/${collection}`)).json()) as { data: ContentType }).data;
}

/** A content type with the ETag (`"v<version>"`) the next change names in `If-Match`. */
export interface LoadedType {
	type: ContentType;
	etag: string;
}

async function loadedType(response: Response): Promise<LoadedType> {
	const { data } = (await response.json()) as { data: ContentType };
	return { type: data, etag: response.headers.get('etag') ?? `"v${data.version}"` };
}

/** Owners and developers only. `slug` names the collection in URLs and can't change. */
export async function createContentType(project: string, slug: string, definition: Definition, key: string): Promise<LoadedType> {
	return loadedType(await send('POST', `${base(project)}/content-types`, { slug, definition }, { 'Idempotency-Key': key }));
}

/** A new version of the content type, on top of the version `etag` names. Refused with
 *  `needs_migration` if stored drafts or publications wouldn't fit it; the error's `details`
 *  name them (D-055). */
export async function updateContentType(
	project: string,
	slug: string,
	definition: Definition,
	etag: string,
	key: string
): Promise<LoadedType> {
	return loadedType(
		await send('PUT', `${base(project)}/content-types/${slug}`, definition, { 'If-Match': etag, 'Idempotency-Key': key })
	);
}

export async function listEntries(project: string, collection: string): Promise<Summary[]> {
	return (await searchEntries(project, collection)).data;
}

/** Up to 100 entries, newest first; `q` searches draft titles (at most 100 characters);
 *  `createdBy` keeps one person's entries, within what the role may list. */
export async function searchEntries(
	project: string,
	collection: string,
	q = '',
	cursor: string | null = null,
	createdBy: string | null = null
): Promise<Page<Summary>> {
	const query = new URLSearchParams({ limit: '100' });
	if (q.trim() !== '') query.set('q', q.trim());
	if (cursor !== null) query.set('cursor', cursor);
	if (createdBy !== null) query.set('created_by', createdBy);
	return (await (await send('GET', `${entries(project, collection)}?${query}`)).json()) as Page<Summary>;
}

/** One revision with its content, for comparing versions. */
export async function getRevision(
	project: string,
	collection: string,
	entry: string,
	revision: string
): Promise<{ revision: Revision; content: Content }> {
	const path = `${entries(project, collection)}/${entry}/revisions/${revision}`;
	return ((await (await send('GET', path)).json()) as { data: { revision: Revision; content: Content } }).data;
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

/** A role, by its key: one of the four every project starts with, or one its owners defined
 *  (D-042, D-058). */
export type Role = string;

/** A project's role, as Settings shows and edits it. */
export interface RoleDefinition {
	key: string;
	name: string;
	description: string;
	built_in: boolean;
	/** The owner role: every permission, never changed or deleted. */
	fixed: boolean;
	version: number;
	permissions: Grant[];
	members: number;
	invitations: number;
	updated_at: string;
}

/** What creating or changing a role sends. */
export interface RoleInput {
	name: string;
	description: string;
	permissions: Grant[];
}

const roles = (project: string) => `/api/v1/projects/${project}/roles`;

export async function listRoles(project: string): Promise<RoleDefinition[]> {
	return ((await (await send('GET', roles(project))).json()) as { data: RoleDefinition[] }).data;
}

export async function createRole(project: string, key: string, role: RoleInput): Promise<RoleDefinition> {
	return ((await (await send('POST', roles(project), { key, ...role })).json()) as { data: RoleDefinition }).data;
}

/** Refused with a conflict when someone changed the role since `version`. */
export async function updateRole(project: string, key: string, version: number, role: RoleInput): Promise<RoleDefinition> {
	const response = await send('PUT', `${roles(project)}/${key}`, role, { 'If-Match': `"v${version}"` });
	return ((await response.json()) as { data: RoleDefinition }).data;
}

/** Refused while members hold the role or pending invitations offer it. */
export async function deleteRole(project: string, key: string, version: number): Promise<void> {
	await send('DELETE', `${roles(project)}/${key}`, undefined, { 'If-Match': `"v${version}"` });
}

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
	role_name: string;
	role_description: string;
	invited_by: string | null;
	expires_at: string;
}

const members = (project: string) => `/api/v1/projects/${project}/members`;
const invitations = (project: string) => `/api/v1/projects/${project}/invitations`;

export async function listMembers(project: string): Promise<Member[]> {
	return ((await (await send('GET', members(project))).json()) as { data: Member[] }).data;
}

/** Needs `members.manage` and every power of both roles; refused when it would leave the project
 *  without an owner. */
export async function changeRole(project: string, identity: string, role: Role): Promise<void> {
	await send('PUT', `${members(project)}/${identity}`, { role });
}

/** Someone who manages members removing someone, or anyone removing themselves (leaving). */
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
	/** The revision readers get now, if published: what this item changes from. */
	published_revision_id: string | null;
	/** Who created the entry, for permissions limited to one's own entries. */
	created_by: string;
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
	/** Who made it, and who sent it for review. */
	created_by: string;
	submitted_by: string | null;
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

/** A scheduled publish (D-060): what its author chose, the instant that resolves to, and how it
 *  went. */
export interface Schedule {
	id: string;
	changeset_id: string;
	title: string;
	changeset_hash: string;
	/** As chosen: `2026-11-03T09:00:00`, in `time_zone`. */
	local_time: string;
	time_zone: string;
	choice?: 'earlier' | 'later';
	run_at: string;
	state: 'scheduled' | 'published' | 'blocked' | 'cancelled';
	reason?: string;
	generation?: number;
	started_at?: string;
	finished_at?: string;
	/** It started more than a minute late, and says so. */
	delayed: boolean;
	scheduled_by: string;
	scheduled_by_name?: string;
	created_at: string;
}

export interface ScheduleRequest {
	local_time: string;
	time_zone: string;
	choice?: 'earlier' | 'later';
}

const schedules = (project: string) => `${base(project)}/schedules`;

/** Schedules an approved changeset at the version on screen; refused with `candidates` when the
 *  local time needs a choice. */
export async function scheduleChangeset(project: string, id: string, hash: string, request: ScheduleRequest): Promise<Schedule> {
	const response = await send('POST', `${changesets(project)}/${id}/schedule`, request, { 'If-Match': `"${hash}"` });
	return ((await response.json()) as { data: Schedule }).data;
}

/** The environment's schedules, pending first; or one changeset's. */
export async function listSchedules(project: string, changeset?: string): Promise<Schedule[]> {
	const query = changeset === undefined ? '' : `?changeset=${encodeURIComponent(changeset)}`;
	return ((await (await send('GET', `${schedules(project)}${query}`)).json()) as { data: Schedule[] }).data;
}

/** Cancels a pending schedule; one that already ran stays as it went. */
export async function cancelSchedule(project: string, id: string): Promise<Schedule> {
	return ((await (await send('DELETE', `${schedules(project)}/${id}`)).json()) as { data: Schedule }).data;
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

/** A project's API token (step R.4): a key a script or a site uses instead of someone's session. */
export interface ApiToken {
	id: string;
	/** The identity its requests act as, so what it writes is attributed to it. */
	identity_id: string;
	name: string;
	description: string;
	kind: 'read-only' | 'full-access' | 'custom';
	permissions: Grant[];
	state: 'active' | 'expired' | 'revoked';
	created_by: { id: string; name: string | null };
	created_at: string;
	expires_at: string;
	last_used_at: string | null;
	revoked_at: string | null;
	/** Only in the answer that made it: the server keeps a hash. */
	secret?: string;
}

export interface NewToken {
	name: string;
	description?: string;
	kind: ApiToken['kind'];
	/** Custom tokens only. */
	permissions?: Grant[];
	/** 1 to 366. */
	expires_in_days: number;
}

const tokens = (project: string) => `/api/v1/projects/${project}/tokens`;

export async function listTokens(project: string): Promise<ApiToken[]> {
	return ((await (await send('GET', tokens(project))).json()) as { data: ApiToken[] }).data;
}

/** Refused with 403 `cannot_grant` for powers the caller's role lacks, 422 `not_for_tokens` for a
 *  people-only action, `invalid_expiry`, `permissions_required` or `too_many_tokens`. */
export async function createToken(project: string, token: NewToken): Promise<ApiToken> {
	return ((await (await send('POST', tokens(project), token)).json()) as { data: ApiToken }).data;
}

/** Revokes it at once; revoking again changes nothing. */
export async function revokeToken(project: string, id: string): Promise<ApiToken> {
	return ((await (await send('DELETE', `${tokens(project)}/${id}`)).json()) as { data: ApiToken }).data;
}

/** One change in a project's audit log (step R.4). */
export interface AuditEvent {
	id: string;
	at: string;
	actor: { id: string; name: string | null; kind: 'person' | 'token' };
	action: string;
	/** A role's key, a member's identity id, an invitation's or a token's id. */
	subject: string;
	detail: Record<string, unknown>;
}

/** Newest first; `action` and `actor` filter exactly. Needs `audit.read`. */
export async function listAudit(
	project: string,
	options: { cursor?: string | null; action?: string; actor?: string; limit?: number } = {}
): Promise<Page<AuditEvent>> {
	const query = new URLSearchParams({ limit: String(options.limit ?? 50) });
	if (options.cursor) query.set('cursor', options.cursor);
	if (options.action) query.set('action', options.action);
	if (options.actor) query.set('actor', options.actor);
	return (await (await send('GET', `/api/v1/projects/${project}/audit?${query}`)).json()) as Page<AuditEvent>;
}
