// A small world for the site-preview journeys (site-preview.spec.ts), so they run without a
// colophon-server: every API call the entry editor and Settings make is answered here (the preview
// management routes as D-171 says), and the editor's own site is
// a stub page served by route fulfilment that behaves like a site using the SDK's preview helper —
// it says `colophon:preview-ready` to the Studio, and shows each `colophon:draft-saved` it hears.

import type { Page, Request, Route } from '@playwright/test';

export const PROJECT = '0b9a3c52-6f0e-4d7e-9a51-3f1c2d4e5a60';
export const ENTRY = '5f2c1a7e-3b4d-4e6f-8a9b-0c1d2e3f4a5b';
export const SITE = 'https://site.preview.test';
const ELSEWHERE = 'https://elsewhere.preview.test';
export const TEMPLATE = `${SITE}/api/preview?code={code}&path=/blog/{slug}`;
const P = `/api/v1/projects/${PROJECT}/environments/production`;
export const EDITOR_URL = `/studio/projects/${PROJECT}/articles/${ENTRY}`;
export const SETTINGS_URL = `/studio/projects/${PROJECT}/settings/connections`;
export const CHANGESET = '7a1d9e40-2c3b-4f5a-8e6d-1b2c3d4e5f60';
export const CHANGESET_URL = `/studio/projects/${PROJECT}/changesets/${CHANGESET}`;

/** How the site behaves when the preview opens in it. */
export type SiteMode =
	/** Says it is showing the draft. */
	| 'ready'
	/** Loads, but its cookie didn't stick in the frame: `previewing: false`. */
	| 'cookieless'
	/** Says nothing (no SDK helper on the page). */
	| 'silent'
	/** Says nothing itself; frames inside it, from elsewhere and from the site, send lookalike
	 *  messages the Studio must ignore. */
	| 'impostor';

export interface World {
	role: 'owner' | 'editor';
	/** What built the request (D-097, D-184): a rollback restores `rollback_generation`; an undo of
	 *  release N sets `undo_of` N and `rollback_generation` N - 1. */
	rollbackGeneration: number | null;
	undoOf: number | null;
	template: string | null;
	/** What issuing a grant answers: a grant, or a refusal. */
	issue: 'grant' | 'not-configured' | 'unavailable' | 'limit' | 'invalid' | 'already-live' | 'offline';
	/** Where the publish request stands. */
	request: 'awaiting_review' | 'published';
	/** What each `POST {P}/previews` asked for, as its body. */
	issued: Record<string, unknown>[];
	/** Milliseconds before an issued grant ends. */
	lasts: number;
	renew: 'ok' | 'gone';
	code: 'ok' | 'gone';
	save: 'ok' | 'fail';
	site: SiteMode;
	/** Holds `POST {P}/previews` until released (to see Opening). */
	hold: Promise<void> | null;
	title: string;
	/** The draft's content, as the last save stored it. */
	content: Record<string, unknown>;
	revision: number;
	grants: number;
	codes: number;
	/** Management calls made, as "METHOD /path-after-{P}". */
	calls: string[];
	/** Codes the site was opened with. */
	opened: string[];
	/** Requests nothing here answers (a test fails on any). */
	unanswered: string[];
}

export function world(overrides: Partial<World> = {}): World {
	return {
		role: 'owner',
		rollbackGeneration: null,
		undoOf: null,
		template: TEMPLATE,
		issue: 'grant',
		request: 'awaiting_review',
		issued: [],
		lasts: 12.5 * 60_000,
		renew: 'ok',
		code: 'ok',
		save: 'ok',
		site: 'ready',
		hold: null,
		title: 'Hello, preview',
		content: { title: 'Hello, preview' },
		revision: 1,
		grants: 0,
		codes: 0,
		calls: [],
		opened: [],
		unanswered: [],
		...overrides
	};
}

const ACTIONS = [
	'content.read',
	'content.create',
	'content.update',
	'content.delete',
	'content.review',
	'content.publish',
	'schema.change',
	'members.manage',
	'roles.manage',
	'destinations.manage',
	'tokens.manage',
	'audit.read',
	'audience.manage'
];
const EDITOR_ACTIONS = ['content.read', 'content.create', 'content.update'];
const AT = '2026-11-17T12:00:00Z';

const TYPE = {
	slug: 'articles',
	version: 1,
	definition: {
		name: 'Articles',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
			{ id: 'body', label: 'Body', type: 'rich_text', required: false }
		]
	},
	definition_hash: 'type-hash-1',
	created_at: AT
};

function revision(w: World) {
	return { id: `rev-${w.revision}`, base_revision_id: null, author_id: 'person-1', created_at: AT, content_hash: `content-${w.revision}`, title: w.title };
}

function draft(w: World, extra: Record<string, unknown> = {}) {
	return {
		id: ENTRY,
		collection: 'articles',
		created_at: AT,
		created_by: 'person-1',
		updated_at: new Date().toISOString(),
		revision: revision(w),
		content: w.content,
		locale: null,
		localizations: [],
		...extra
	};
}

function changeset(w: World) {
	const published = w.request === 'published';
	return {
		id: CHANGESET,
		title: 'Spring issue',
		hash: 'sha256:request-hash-1',
		hidden_items: 0,
		status: { state: w.request, generation: published ? 3 : null, by: null, self_review: false, edited_items: 0 },
		created_by: 'person-1',
		created_at: AT,
		submitted_by: 'person-1',
		submitted_at: AT,
		updated_at: AT,
		rollback_generation: w.rollbackGeneration,
		undo_of: w.undoOf,
		items: [
			{
				kind: 'publish',
				entry_id: ENTRY,
				collection: 'articles',
				revision_id: `rev-${w.revision}`,
				title: w.title,
				content_hash: 'sha256:content-hash-1',
				schema_version: 1,
				current_revision_id: `rev-${w.revision}`,
				edited: false,
				published_revision_id: null,
				created_by: 'person-1'
			}
		],
		reviews: [],
		stages: []
	};
}

function project(w: World) {
	return {
		id: PROJECT,
		slug: 'the-draft',
		name: 'The Draft',
		role: w.role,
		role_name: w.role === 'owner' ? 'Owner' : 'Editor',
		permissions: (w.role === 'owner' ? ACTIONS : EDITOR_ACTIONS).map((action) => ({ action, subject: '*' })),
		self_review: true
	};
}

function originOf(template: string | null): string | null {
	if (template === null) return null;
	try {
		return new URL(template.replace(/\{[^{}]*\}/g, 'x')).origin;
	} catch {
		return null;
	}
}

function fill(template: string, code: string): string {
	return template
		.replaceAll('{code}', encodeURIComponent(code))
		.replaceAll('{collection}', 'articles')
		.replaceAll('{id}', ENTRY)
		.replaceAll('{slug}', 'hello-preview')
		.replaceAll('{locale}', '');
}

const refusal = (status: number, reason: string, message: string) => ({ status, body: { error: { code: reason, message, details: { reason } } } });

/** The answer to one Studio API request, or null when nothing here answers it. */
async function answer(w: World, request: Request): Promise<{ status: number; body?: unknown; headers?: Record<string, string> } | null> {
	const url = new URL(request.url());
	const path = url.pathname;
	const method = request.method();
	const ok = (body: unknown, headers: Record<string, string> = {}) => ({ status: 200, body, headers });

	if (path === '/me') {
		return ok({
			data: {
				id: 'person-1',
				issuer: 'colophon',
				subject: 'person-1',
				display_name: 'Test editor',
				administrator: false,
				installation: { has_administrator: true },
				projects: [{ id: PROJECT, slug: 'the-draft', name: 'The Draft', role: w.role, role_name: w.role === 'owner' ? 'Owner' : 'Editor' }]
			}
		});
	}
	if (path === `/api/v1/projects/${PROJECT}`) return ok({ data: project(w) });
	if (path === `/api/v1/projects/${PROJECT}/members`) return ok({ data: [] });
	if (!path.startsWith(P)) return null;
	const rest = path.slice(P.length);

	// The preview management routes (the contract).
	if (rest === '/preview-target' && method === 'GET') {
		w.calls.push('GET /preview-target');
		return ok({ data: { url_template: w.template, origin: originOf(w.template), updated_at: w.template === null ? null : AT } });
	}
	if (rest === '/preview-target' && method === 'PUT') {
		w.calls.push('PUT /preview-target');
		const { url_template } = request.postDataJSON() as { url_template: string | null };
		if (url_template !== null && url_template.includes('refuse-me')) {
			return refusal(422, 'invalid_preview_url', 'the preview address must point at a public site');
		}
		w.template = url_template;
		return ok({ data: { url_template: w.template, origin: originOf(w.template), updated_at: new Date().toISOString() } });
	}
	if (rest === '/previews' && method === 'POST') {
		w.calls.push('POST /previews');
		const asked = request.postDataJSON() as Record<string, unknown>;
		w.issued.push(asked);
		if (w.hold !== null) await w.hold;
		if (w.issue === 'offline') return { status: -1 };
		if (w.issue === 'already-live') return refusal(409, 'already_published', 'this publish request is already live; open the site itself');
		if (w.issue === 'not-configured' || w.template === null) return refusal(422, 'preview_not_configured', 'preview is not configured for this environment');
		if (w.issue === 'unavailable') return refusal(503, 'unavailable', 'the database is unavailable');
		if (w.issue === 'limit') return refusal(429, 'preview_limit', 'too many live previews in this project');
		if (w.issue === 'invalid') return refusal(422, 'invalid_preview_url', 'the preview address no longer works');
		w.grants += 1;
		const code = `cpc_grant${w.grants}`;
		return {
			status: 201,
			body: {
				data: {
					id: `grant-${w.grants}`,
					code,
					code_expires_at: new Date(Date.now() + 60_000).toISOString(),
					expires_at: new Date(Date.now() + w.lasts).toISOString(),
					url: fill(w.template, code),
					origin: originOf(w.template),
					scope: typeof asked['changeset'] === 'string' ? { kind: 'changeset', changeset: asked['changeset'] } : { kind: 'entry', entry: ENTRY, collection: 'articles' }
				}
			}
		};
	}
	const grant = /^\/previews\/([^/]+)(\/renew|\/code)?$/.exec(rest);
	if (grant !== null) {
		const [, id, action] = grant;
		w.calls.push(`${method} /previews/${id}${action ?? ''}`);
		if (method === 'DELETE') return { status: 204 };
		if (action === '/renew') {
			return w.renew === 'gone' ? refusal(404, 'preview_not_found', 'not found') : ok({ data: { id, expires_at: new Date(Date.now() + 15 * 60_000).toISOString() } });
		}
		if (action === '/code') {
			if (w.code === 'gone') return refusal(404, 'preview_not_found', 'not found');
			w.codes += 1;
			const code = `cpc_fresh${w.codes}`;
			return { status: 201, body: { data: { code, code_expires_at: new Date(Date.now() + 60_000).toISOString(), url: fill(w.template ?? TEMPLATE, code) } } };
		}
	}

	// What the entry editor and the project's frame read.
	if (rest === '/content-types') return ok({ data: [TYPE] });
	if (rest === '/content-types/articles') return ok({ data: TYPE });
	if (rest === '/changesets') return ok({ data: [] });
	if (rest === `/changesets/${CHANGESET}`) return ok({ data: changeset(w) });
	if (rest === '/schedules') return ok({ data: [] });
	// The request page reads its review tasks to say who it waits for (P2-12).
	if (rest === '/tasks') return ok({ data: [], next_cursor: null });
	if (rest.startsWith(`/collections/articles/entries/${ENTRY}/revisions/`)) {
		return ok({ data: { revision: revision(w), content: w.content } });
	}
	if (rest === '/releases') return ok({ data: [] }, { 'colophon-generation': '0' });
	if (rest === '/events') return { status: 204 };
	if (rest === '/stages') return ok({ data: [] }, { etag: '"stages-1"' });
	if (rest === '/destinations') return ok({ data: [] });
	if (rest === '/collections/articles/entries') {
		const { revision: rev, updated_at, created_by } = draft(w);
		return ok({ data: [{ id: ENTRY, created_by, updated_at, revision: rev, published: null, locale: null }], next_cursor: null });
	}
	if (rest === `/collections/articles/entries/${ENTRY}` && method === 'GET') {
		return ok({ data: draft(w, { published: null, referenced_by: [] }) }, { etag: `"rev-${w.revision}"` });
	}
	if (rest === `/collections/articles/entries/${ENTRY}` && method === 'PUT') {
		if (w.save === 'fail') return refusal(500, 'internal', 'internal error');
		const content = request.postDataJSON() as Record<string, unknown>;
		w.revision += 1;
		w.content = content;
		w.title = typeof content['title'] === 'string' ? content['title'] : w.title;
		return ok({ data: draft(w) }, { etag: `"rev-${w.revision}"` });
	}
	if (rest === `/collections/articles/entries/${ENTRY}/revisions`) return ok({ data: [revision(w)] });
	if (rest.startsWith(`/collections/articles/entries/${ENTRY}/comments`)) return ok({ data: [], next_cursor: null });
	return null;
}

/** The stub site: a page in preview mode, as a site using the SDK's preview helper serves it. */
function sitePage(w: World, studio: string, code: string): string {
	const say =
		w.site === 'ready' || w.site === 'cookieless'
			? `parent.postMessage({ type: 'colophon:preview-ready', previewing: ${w.site === 'ready'} }, ${JSON.stringify(studio)});`
			: '';
	const impostors =
		w.site === 'impostor'
			? `<iframe title="Somebody else's widget" src="${ELSEWHERE}/widget" style="width:1px;height:1px;border:0"></iframe>
			   <iframe title="A widget on this site" src="${SITE}/widget" style="width:1px;height:1px;border:0"></iframe>`
			: '';
	return `<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>${w.title} · Example site</title>
<style>
	body { margin: 0; font: 16px/1.5 system-ui, sans-serif; color: #1c1917; background: #fff; }
	header { display: flex; align-items: center; justify-content: space-between; gap: 1rem; padding: 0.75rem 1.25rem; border-bottom: 1px solid #e7e5e4; }
	.mark { font-weight: 700; letter-spacing: -0.01em; }
	.mode { font-size: 0.75rem; font-weight: 600; padding: 0.15rem 0.6rem; border-radius: 999px; background: #fef3c7; color: #78350f; }
	main { max-width: 42rem; margin: 0 auto; padding: 2rem 1.25rem; }
	h1 { font-size: clamp(1.75rem, 5vw, 2.5rem); line-height: 1.15; margin: 0 0 0.75rem; }
	p { color: #44403c; }
	.hero { height: 10rem; border-radius: 0.75rem; background: linear-gradient(135deg, #e7e5e4, #d6d3d1); margin: 1.5rem 0; }
</style>
</head>
<body>
<header><span class="mark">Example site</span><span class="mode">${w.site === 'cookieless' ? 'Published version' : 'Preview mode'}</span></header>
<main>
	<h1 id="title">${w.title}</h1>
	<p id="saved" data-code="${code}">Showing the draft as last saved.</p>
	<div class="hero" role="img" aria-label="A placeholder picture"></div>
	<p>The site renders this page with the preview token its route got for the code, so it shows the draft, not what readers see.</p>
</main>
${impostors}
<script>
	window.addEventListener('message', (event) => {
		if (event.origin !== ${JSON.stringify(studio)}) return;
		if (event.data && event.data.type === 'colophon:draft-saved') {
			document.getElementById('saved').textContent = 'Updated to version ' + event.data.revision + ' of ' + event.data.entry;
		}
	});
	${say}
</script>
</body>
</html>`;
}

/** A frame that pretends: it sends the Studio a "ready" from where it shouldn't. */
function impostorPage(studio: string): string {
	return `<!doctype html><html lang="en"><head><meta charset="utf-8"><title>Widget</title></head><body>
<script>window.top.postMessage({ type: 'colophon:preview-ready', previewing: true }, ${JSON.stringify(studio)});</script>
</body></html>`;
}

/** Answers the Studio's API from `w`, and serves the site, in `page`'s context (new tabs too). */
export async function serve(page: Page, w: World, studio: string): Promise<void> {
	await page.context().route(
		(url) => url.pathname === '/me' || url.pathname.startsWith('/api/'),
		async (route: Route) => {
			const found = await answer(w, route.request());
			if (found !== null && found.status === -1) {
				await route.abort('internetdisconnected');
				return;
			}
			if (found === null) {
				w.unanswered.push(`${route.request().method()} ${new URL(route.request().url()).pathname}`);
				await route.fulfill({ status: 404, json: { error: { code: 'not_found', message: 'not found', details: { reason: 'not_found' } } } });
				return;
			}
			await route.fulfill({
				status: found.status,
				headers: { 'content-type': 'application/json', ...(found.headers ?? {}) },
				...(found.body === undefined ? {} : { body: JSON.stringify(found.body) })
			});
		}
	);
	await page.context().route(`${SITE}/**`, async (route) => {
		const url = new URL(route.request().url());
		if (url.pathname === '/widget') {
			await route.fulfill({ status: 200, contentType: 'text/html', body: impostorPage(studio) });
			return;
		}
		const code = url.searchParams.get('code') ?? '';
		w.opened.push(code);
		await route.fulfill({ status: 200, contentType: 'text/html', body: sitePage(w, studio, code) });
	});
	await page.context().route(`${ELSEWHERE}/**`, (route) => route.fulfill({ status: 200, contentType: 'text/html', body: impostorPage(studio) }));
}
