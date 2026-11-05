// `POST /api/signup`: the create-account form on /signin. Same shape as /api/signin — a new
// account in the project's open audience role (`member`), signed in at once.

import type { APIRoute } from 'astro';
import { signUp, site } from '@colophon/site-kit';
import { redirect, setSession } from '../../lib/session.ts';

export const prerender = false;

export const POST: APIRoute = async ({ request }) => {
	const cms = site();
	if (cms === null) return redirect('/');
	const form = await request.formData();
	const email = String(form.get('email') ?? '');
	const password = String(form.get('password') ?? '');
	const result = await signUp(cms, email, password);
	if ('error' in result) {
		return redirect(`/signin/?error=${encodeURIComponent(result.error)}`);
	}
	return redirect('/members/', (response) => setSession(response, result.ok.token));
};
