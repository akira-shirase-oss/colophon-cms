// The site's own people: the public audience API, `/audience/v1/{project}/…` (docs/modules/03-auth.md
// "Audience", D-137), called server-side by the pages that sign visitors in and out. The site
// keeps the session token in its own first-party cookie (`draft_session` on The Draft) and sends
// it as `Authorization: Bearer` — the bearer form needs no CSRF token, and the CMS's cookies
// never have to cross origins.

import type { Site } from './config.ts';

/** A site user as `me` and the session answers show one (never a password hash). */
export interface AudienceUser {
	id: string;
	email: string;
	display_name: string | null;
	role: string;
}

/** What a successful signup or sign-in hands back: the user and its session token. */
export interface Issued {
	user: AudienceUser;
	token: string;
	csrf: string;
}

/** The answer to a credentials call: the session, or the error code the API gave. */
export type CredentialResult = { ok: Issued } | { error: string };

interface AudienceSession {
	user: AudienceUser;
	session: string;
	csrf: string;
}

interface Envelope<T> {
	data?: T;
	error?: { code?: string; details?: { reason?: string } };
}

async function post<T>(site: Site, action: string, body: unknown): Promise<{ status: number; body: Envelope<T> }> {
	const response = await fetch(`${site.api}/audience/v1/${site.project}/${action}`, {
		method: 'POST',
		headers: { 'content-type': 'application/json', accept: 'application/json' },
		body: JSON.stringify(body)
	});
	return { status: response.status, body: (await response.json()) as Envelope<T> };
}

function issued(body: Envelope<AudienceSession>): CredentialResult {
	const made = body.data;
	if (made === undefined) {
		return { error: body.error?.details?.reason ?? body.error?.code ?? 'failed' };
	}
	return { ok: { user: made.user, token: made.session, csrf: made.csrf } };
}

/** `POST /audience/v1/{project}/signup`: a new site user, in the project's open role. */
export async function signUp(site: Site, email: string, password: string, displayName?: string): Promise<CredentialResult> {
	const { body } = await post<AudienceSession>(site, 'signup', { email, password, display_name: displayName });
	return issued(body);
}

/** `POST /audience/v1/{project}/signin`. */
export async function signIn(site: Site, email: string, password: string): Promise<CredentialResult> {
	const { body } = await post<AudienceSession>(site, 'signin', { email, password });
	return issued(body);
}

/** `POST /audience/v1/{project}/signout` as a bearer credential — best effort, never throws. */
export async function signOut(site: Site, token: string): Promise<void> {
	try {
		await fetch(`${site.api}/audience/v1/${site.project}/signout`, {
			method: 'POST',
			headers: { authorization: `Bearer ${token}` }
		});
	} catch {
		// The cookie is cleared either way; a dead session is already signed out.
	}
}

/** `GET /audience/v1/{project}/me`: who the session belongs to, `null` when it is dead. */
export async function me(site: Site, token: string): Promise<AudienceUser | null> {
	const response = await fetch(`${site.api}/audience/v1/${site.project}/me`, {
		headers: { authorization: `Bearer ${token}`, accept: 'application/json' }
	});
	if (response.status === 401) return null;
	if (!response.ok) {
		throw new Error(`GET /audience/v1/…/me: HTTP ${response.status}`);
	}
	return ((await response.json()) as { data?: AudienceUser }).data ?? null;
}
