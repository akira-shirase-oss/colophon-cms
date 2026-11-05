// `POST /api/signin`: the form on /signin. The site calls the CMS's audience sign-in
// server-side, keeps the returned session token in its own cookie and sends it back out as
// `Authorization: Bearer` — the CMS's cookies never leave its origin, so there is no CSRF to
// forward. Failures land back on the form with the API's own reason.

import type { APIRoute } from 'astro';
import { signIn, site } from '@colophon/site-kit';
import { redirect, setSession } from '../../lib/session.ts';

export const prerender = false;

export const POST: APIRoute = async ({ request }) => {
	const cms = site();
	if (cms === null) return redirect('/');
	const form = await request.formData();
	const email = String(form.get('email') ?? '');
	const password = String(form.get('password') ?? '');
	const result = await signIn(cms, email, password);
	if ('error' in result) {
		return redirect(`/signin/?error=${encodeURIComponent(result.error)}`);
	}
	return redirect('/members/', (response) => setSession(response, result.ok.token));
};
