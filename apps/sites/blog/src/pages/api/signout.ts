// `POST /api/signout`: ends the CMS session the cookie names (as a bearer credential — no
// CSRF, it was never ambient on the CMS's origin) and clears the site's cookie either way.

import type { APIRoute } from 'astro';
import { signOut, site } from '@colophon/site-kit';
import { clearSession, redirect, sessionToken } from '../../lib/session.ts';

export const prerender = false;

export const POST: APIRoute = async ({ request }) => {
	const cms = site();
	const token = sessionToken(request);
	if (cms !== null && token !== null) await signOut(cms, token);
	return redirect('/', (response) => clearSession(response));
};
