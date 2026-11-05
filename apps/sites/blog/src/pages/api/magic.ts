// `POST /api/magic`: the "email me a sign-in link" form. The CMS records a single-use link and
// answers the same whether or not the address has an account. With COLOPHON_DEV_MAIL=1 on the
// CMS it also hands the link back, and this route simply follows it — the development stack has
// no mail server, so the "email" is a redirect. In production the reader would check their inbox.

import type { APIRoute } from 'astro';
import { site } from '@colophon/site-kit';
import { redirect } from '../../lib/session.ts';

export const prerender = false;

export const POST: APIRoute = async ({ request, url }) => {
	const cms = site();
	if (cms === null) return redirect('/');
	const form = await request.formData();
	const email = String(form.get('email') ?? '').trim();
	if (email === '') return redirect('/signin/?error=magic_link_invalid');
	const response = await fetch(`${cms.api}/audience/v1/${cms.project}/magic-link`, {
		method: 'POST',
		headers: { 'content-type': 'application/json' },
		body: JSON.stringify({ email, return_to: `${url.origin}/magic` })
	});
	if (!response.ok) return redirect('/signin/?error=failed');
	const body = (await response.json().catch(() => null)) as { data?: { dev_link?: string } } | null;
	const dev = body?.data?.dev_link;
	// Development: follow the link the CMS would have emailed. Production: say it was sent.
	return dev === undefined ? redirect('/signin/?sent=1') : redirect(dev);
};
