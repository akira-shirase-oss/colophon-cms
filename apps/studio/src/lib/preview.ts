// Preview on the editor's own site (D-173): an editor sees a draft on the real site
// before it goes live. The Studio issues a short-lived grant for one entry; the site's preview
// route exchanges its one-time code for a token and reads the draft with it. Colophon never
// relies on a cookie inside the frame: the site keeps its own (partitioned) cookie, and says
// whether it stuck (`colophon:preview-ready`, `previewing`).
//
// This module holds the management calls (through api.ts's one fetch layer, so writes carry the
// CSRF token and every failure is a typed ApiError) and the pure rules the Studio's preview pane
// follows: the address template's checks (the server's, mirrored), the pane's state machine, the
// exact origin check on messages, and the words for when the preview ends.

import { ApiError, base, csrfToken, send } from './api.ts';
import { describe, plainOr, type Said } from './errors.ts';

// ---------------------------------------------------------------------------------------------
// The API (`{P}/preview-target`, `{P}/previews`)
// ---------------------------------------------------------------------------------------------

/** Where an environment's previews open: an address template with `{code}` in it, or none. */
export interface PreviewTarget {
	url_template: string | null;
	/** The template's origin: the only origin the Studio talks to. */
	origin: string | null;
	updated_at: string | null;
}

/** What a grant may read: one entry (and its translations), or one publish request. */
export interface PreviewScope {
	kind: 'entry' | 'changeset';
	entry?: string;
	changeset?: string;
	collection?: string;
}

/** A preview grant, as issued: the one-time code and the address that carries it. */
export interface PreviewGrant {
	id: string;
	code: string;
	code_expires_at: string;
	expires_at: string;
	/** The template with its placeholders filled: what the frame (or a new tab) opens. */
	url: string;
	/** The exact `targetOrigin` for messages to the site, and the only origin heard from it. */
	origin: string;
	scope: PreviewScope;
}

/** A fresh one-time code for a live grant, and the address that carries it. */
export interface PreviewCode {
	code: string;
	code_expires_at: string;
	url: string;
}

/** One of the caller's own live grants. */
export interface LivePreview {
	id: string;
	scope: PreviewScope;
	created_at: string;
	expires_at: string;
	exchanged: boolean;
}

const targetPath = (project: string) => `${base(project)}/preview-target`;
const previewsPath = (project: string) => `${base(project)}/previews`;

async function data<T>(response: Response): Promise<T> {
	return ((await response.json()) as { data: T }).data;
}

/** Any member who reads content may ask, so the editor knows whether Preview is set up. */
export async function getPreviewTarget(project: string): Promise<PreviewTarget> {
	return data(await send('GET', targetPath(project)));
}

/** Owners and developers (`destinations.manage`); `null` turns preview off. Refused with 422
 *  `invalid_preview_url` and a plain message when the template won't do. */
export async function setPreviewTarget(project: string, urlTemplate: string | null): Promise<PreviewTarget> {
	return data(await send('PUT', targetPath(project), { url_template: urlTemplate }));
}

/** Issues a grant for one entry. Refused with 422 `preview_not_configured` when no template is
 *  set, 404 for an entry the caller can't read, and 429 `preview_limit` at 1,000 live grants. */
export async function issuePreview(project: string, entry: string, ttlSeconds?: number): Promise<PreviewGrant> {
	return data(await send('POST', previewsPath(project), { entry, ...(ttlSeconds === undefined ? {} : { ttl_seconds: ttlSeconds }) }));
}

/** The caller's own live grants for one entry. */
export async function listPreviews(project: string, entry: string): Promise<LivePreview[]> {
	return data(await send('GET', `${previewsPath(project)}?entry=${encodeURIComponent(entry)}`));
}

/** Pushes a grant's end out again (capped 8 hours after it was made); 404 once it has ended. */
export async function renewPreview(project: string, id: string): Promise<{ id: string; expires_at: string }> {
	return data(await send('POST', `${previewsPath(project)}/${encodeURIComponent(id)}/renew`));
}

/** A fresh one-time code for the same grant, to reload the frame through the site's route. */
export async function previewCode(project: string, id: string): Promise<PreviewCode> {
	return data(await send('POST', `${previewsPath(project)}/${encodeURIComponent(id)}/code`));
}

/** Ends a grant now: the site's next read with its token is refused. */
export async function revokePreview(project: string, id: string): Promise<void> {
	await send('DELETE', `${previewsPath(project)}/${encodeURIComponent(id)}`);
}

/** Ends a grant as the page goes away (a closed tab, a reload): a request the browser finishes
 *  after the page is gone. Best effort — an unrevoked grant still ends on its own. */
export function revokePreviewOnExit(project: string, id: string): void {
	try {
		void fetch(`${previewsPath(project)}/${encodeURIComponent(id)}`, {
			method: 'DELETE',
			keepalive: true,
			headers: { Accept: 'application/json', 'X-CSRF-Token': csrfToken() }
		}).catch(() => {});
	} catch {
		// Nothing to do: the grant expires by itself.
	}
}

// ---------------------------------------------------------------------------------------------
// The address template (Settings → Website connections → Preview on your site)
// ---------------------------------------------------------------------------------------------

/** The placeholders Colophon fills in a preview address. `{code}` is required. */
export const PLACEHOLDERS = ['code', 'collection', 'id', 'slug', 'locale'] as const;
/** The longest template the server takes. */
export const MAX_TEMPLATE = 2048;
export const TEMPLATE_EXAMPLE = 'https://www.example.com/api/preview?code={code}&path=/blog/{slug}';

const PLACEHOLDER = /\{([^{}]*)\}/g;

/** The template with each placeholder replaced by `fill(name)`. */
function filled(template: string, fill: (name: string) => string): string {
	return template.replace(PLACEHOLDER, (_, name: string) => fill(name));
}

/** Plain http is for a site on this computer only. */
function localHost(hostname: string): boolean {
	return hostname === 'localhost' || hostname === '127.0.0.1';
}

/**
 * What's wrong with a preview address template, in words a developer can act on; `''` when the
 * server would take it. Mirrors the server's checks: https (plain http only for localhost),
 * at most 2,048 characters, `{code}` somewhere, only Colophon's placeholders, and — so there is
 * one origin to talk to — none in the site's own address.
 */
export function templateProblem(text: string): string {
	const template = text.trim();
	if (template === '') return 'Write the address of your site’s preview route.';
	if ([...template].length > MAX_TEMPLATE) return `This address is too long: at most ${MAX_TEMPLATE.toLocaleString('en')} characters.`;
	const unknown = [...template.matchAll(PLACEHOLDER)].map((match) => match[1] ?? '').find((name) => !(PLACEHOLDERS as readonly string[]).includes(name));
	if (unknown !== undefined) return `{${unknown}} isn’t a placeholder Colophon fills. Use {code}, {collection}, {id}, {slug} or {locale}.`;
	if (/[{}]/.test(template.replace(PLACEHOLDER, ''))) return 'Curly brackets are only for placeholders, like {code}. Check for one left open.';
	if (!template.includes('{code}')) return 'Add {code} where your site’s preview route expects the code, like ?code={code}.';
	let first: URL;
	let second: URL;
	try {
		first = new URL(filled(template, (name) => `colophon-${name}-a`));
		second = new URL(filled(template, (name) => `colophon-${name}-b`));
	} catch {
		return `That isn’t a full web address. Start it with https://, like ${TEMPLATE_EXAMPLE}.`;
	}
	if (first.protocol !== 'https:' && !(first.protocol === 'http:' && localHost(first.hostname))) {
		return 'Use an https:// address. Plain http:// works only for localhost while you develop.';
	}
	if (first.origin !== second.origin) return 'Placeholders go in the path or the query, not in the site’s own address.';
	if (first.username !== '' || first.password !== '') return 'Leave the user name and password out of the address.';
	return '';
}

/** The origin a valid template opens on, or `null`. */
export function templateOrigin(text: string): string | null {
	if (templateProblem(text) !== '') return null;
	return new URL(filled(text.trim(), (name) => `colophon-${name}`)).origin;
}

// ---------------------------------------------------------------------------------------------
// The pane's state machine
// ---------------------------------------------------------------------------------------------

/** How long the site has to say it loaded the preview before the pane says it can't tell. */
export const QUIET_MS = 10_000;
/** How often an open preview pushes its end out again. */
export const RENEW_EVERY_MS = 4 * 60_000;
/** Renew sooner than that when the end is this close (a laptop woke up, a timer was held back). */
export const RENEW_WITHIN_MS = 3 * 60_000;
/** At or under this width, side by side becomes two tabs: Edit and Preview. */
export const NARROW_QUERY = '(max-width: 640px)';

export type PaneState =
	| { kind: 'closed' }
	/** Issuing the grant (`grant` null), or waiting for the site to say it loaded. */
	| { kind: 'opening'; grant: PreviewGrant | null }
	/** The site said it is showing the draft. */
	| { kind: 'live'; grant: PreviewGrant }
	/** The site loaded but holds no preview session: its cookie was kept out of the frame. */
	| { kind: 'cookie-blocked'; grant: PreviewGrant }
	/** No word from the site within QUIET_MS. The frame still shows whatever it loaded. */
	| { kind: 'unconfirmed'; grant: PreviewGrant }
	/** The grant ended (expired or revoked) before the pane closed. */
	| { kind: 'ended' }
	/** Nobody set the preview address (422 `preview_not_configured`). */
	| { kind: 'not-set-up' }
	/** Issuing (or a fresh code) failed; `grant` is kept so closing still revokes it. */
	| { kind: 'failed'; error: unknown; grant: PreviewGrant | null };

export type PaneEvent =
	| { type: 'open' }
	| { type: 'issued'; grant: PreviewGrant }
	| { type: 'failed'; error: unknown }
	| { type: 'ready'; previewing: boolean }
	| { type: 'quiet' }
	| { type: 'reload' }
	| { type: 'ended' }
	| { type: 'close' };

/** The grant a state holds, if any. */
export function grantOf(state: PaneState): PreviewGrant | null {
	return 'grant' in state ? state.grant : null;
}

/** Whether `error` says the preview address isn't set. */
export function notConfigured(error: unknown): boolean {
	return error instanceof ApiError && error.status === 422 && error.reason === 'preview_not_configured';
}

/** The pane's next state. Events that don't apply to a state leave it as it is. */
export function step(state: PaneState, event: PaneEvent): PaneState {
	const grant = grantOf(state);
	switch (event.type) {
		case 'open':
			return state.kind === 'closed' || state.kind === 'ended' || state.kind === 'not-set-up' || (state.kind === 'failed' && grant === null)
				? { kind: 'opening', grant: null }
				: state;
		case 'issued':
			return state.kind === 'opening' ? { kind: 'opening', grant: event.grant } : state;
		case 'failed':
			if (state.kind !== 'opening') return state;
			if (notConfigured(event.error)) return { kind: 'not-set-up' };
			// A fresh code for a grant that has ended: the preview is over, not broken.
			if (grant !== null && event.error instanceof ApiError && event.error.failure === 'not-found') return { kind: 'ended' };
			return { kind: 'failed', error: event.error, grant };
		case 'ready':
			if (grant === null || state.kind === 'failed') return state;
			return event.previewing ? { kind: 'live', grant } : { kind: 'cookie-blocked', grant };
		case 'quiet':
			return state.kind === 'opening' && grant !== null ? { kind: 'unconfirmed', grant } : state;
		case 'reload':
			return grant === null ? state : { kind: 'opening', grant };
		case 'ended':
			return state.kind === 'closed' ? state : { kind: 'ended' };
		case 'close':
			return { kind: 'closed' };
	}
}

// ---------------------------------------------------------------------------------------------
// Messages between the Studio and the site (postMessage)
// ---------------------------------------------------------------------------------------------

/** What the Studio tells the site after each save the server acknowledged. */
export interface DraftSaved {
	type: 'colophon:draft-saved';
	entry: string;
	revision: string;
}

export function draftSaved(entry: string, revision: string): DraftSaved {
	return { type: 'colophon:draft-saved', entry, revision };
}

/**
 * `previewing` from a `colophon:preview-ready` message the site sent, or `null` for anything
 * else: the origin must be exactly the grant's, and, when `frame` is given, the sender must be
 * that frame. Every other message is ignored.
 */
export function readyFrom(event: { origin: string; data: unknown; source?: unknown }, origin: string, frame?: unknown): boolean | null {
	if (origin === '' || event.origin !== origin) return null;
	if (frame !== undefined && event.source !== frame) return null;
	const message = event.data;
	if (typeof message !== 'object' || message === null) return null;
	const { type, previewing } = message as { type?: unknown; previewing?: unknown };
	if (type !== 'colophon:preview-ready' || typeof previewing !== 'boolean') return null;
	return previewing;
}

// ---------------------------------------------------------------------------------------------
// Words and timing
// ---------------------------------------------------------------------------------------------

/** "Ends in 12 min": how long the preview lasts if nothing renews it. Rounded down, so it never
 *  promises longer than it has. */
export function endsIn(expiresAt: string, now = Date.now()): string {
	const left = Date.parse(expiresAt) - now;
	if (Number.isNaN(left) || left <= 0) return 'Ended';
	if (left < 60_000) return 'Ends in under a minute';
	return `Ends in ${Math.floor(left / 60_000)} min`;
}

/** Whether an open preview should push its end out now. */
export function renewDue(expiresAt: string, renewedAt: number, now = Date.now()): boolean {
	return now - renewedAt >= RENEW_EVERY_MS || Date.parse(expiresAt) - now <= RENEW_WITHIN_MS;
}

/** Whether a grant's end has passed. */
export function expired(expiresAt: string, now = Date.now()): boolean {
	return Date.parse(expiresAt) <= now;
}

/** The host a preview opens on, for people ("www.example.com"). */
export function hostOf(url: string): string {
	try {
		return new URL(url).host;
	} catch {
		return '';
	}
}

/** The widths the frame can take; `null` fills the pane. */
export const WIDTHS = [
	{ value: '360', px: 360, name: 'Phone' },
	{ value: '768', px: 768, name: 'Tablet' },
	{ value: '1280', px: 1280, name: 'Desktop' },
	{ value: 'fit', px: null, name: 'Fit' }
] as const;
export type Width = (typeof WIDTHS)[number]['value'];

/** How much a frame `px` wide is shrunk to fit `room` pixels (1 when it fits). */
export function scaleFor(px: number | null, room: number): number {
	if (px === null || room <= 0 || px <= room) return 1;
	return Math.max(0.1, Math.floor((room / px) * 100) / 100);
}

/** Why a preview couldn't start (or reload), in the Studio's words; the server's in `technical`. */
export function previewProblem(error: unknown): Said {
	const said = describe(error, 'preview this entry');
	if (!(error instanceof ApiError)) return { ...said, title: 'The preview didn’t open' };
	if (error.reason === 'preview_limit') {
		return {
			title: 'Too many previews open',
			text: 'This project has as many previews open as it allows. Close a preview you no longer need, or wait: each one ends on its own within the hour.',
			technical: said.technical
		};
	}
	if (error.status === 422) {
		return {
			title: 'Your site’s preview address doesn’t work',
			text: `${plainOr(error.message, 'Colophon couldn’t fill in the preview address.')} Check it in Settings, under Website connections.`,
			technical: said.technical
		};
	}
	if (error.failure === 'not-found') {
		return { title: 'This entry can’t be previewed', text: 'It isn’t there any more, or your role no longer lets you see it. Reload the page.', technical: said.technical };
	}
	if (error.failure === 'retry') {
		return {
			title: 'The preview didn’t open',
			text: error.status === null ? 'Colophon didn’t answer. Check your connection, then try again.' : 'Colophon couldn’t start the preview just now. Try again in a moment.',
			technical: said.technical
		};
	}
	return { ...said, title: said.title === 'Not accepted' ? 'The preview didn’t open' : said.title };
}

/**
 * Opens `url` from `issue` in a new tab, first-party (where the site's cookie always sticks).
 * The tab opens at once, inside the click, so no popup blocker stops it, and says what it is
 * waiting for; it is closed again if the address never comes. Returns the failure, if any.
 */
export async function openInNewTab(issue: () => Promise<{ url: string }>): Promise<unknown> {
	const tab = window.open('', '_blank');
	if (tab !== null) {
		try {
			tab.opener = null;
			tab.document.title = 'Opening a preview…';
			tab.document.body.style.font = '16px system-ui, sans-serif';
			tab.document.body.style.margin = '2rem';
			tab.document.body.textContent = 'Opening a private preview of your site…';
		} catch {
			// A browser that won't let us write into the blank tab still navigates it.
		}
	}
	try {
		const { url } = await issue();
		if (tab === null) window.open(url, '_blank', 'noopener,noreferrer');
		else tab.location.replace(url);
		return null;
	} catch (error) {
		tab?.close();
		return error;
	}
}
