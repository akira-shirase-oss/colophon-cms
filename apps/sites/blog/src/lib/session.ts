// The Draft's own session cookie: it holds the audience session token the CMS answered a
// signup or sign-in with (`colophon_audience`'s value), kept server-side and sent to the CMS
// as `Authorization: Bearer`. HttpOnly like the CMS's own; the CMS's cookies stay on its
// origin — the site's reads all go out with the bearer form, which needs no CSRF token.

/** The cookie the members pages and the api routes share. */
export const SESSION_COOKIE = 'draft_session';

/** Thirty days, like an audience session's own bound — the CMS still expires it first. */
const MAX_AGE = 30 * 24 * 60 * 60;

export function setSession(response: Response, token: string): void {
	response.headers.append(
		'Set-Cookie',
		`${SESSION_COOKIE}=${token}; Path=/; HttpOnly; SameSite=Lax; Max-Age=${MAX_AGE}`
	);
}

export function clearSession(response: Response): void {
	response.headers.append('Set-Cookie', `${SESSION_COOKIE}=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0`);
}

/** The token the request carries, if the browser sent one. */
export function sessionToken(request: Request): string | null {
	const header = request.headers.get('cookie') ?? '';
	for (const part of header.split(';')) {
		const [name, ...rest] = part.trim().split('=');
		if (name === SESSION_COOKIE) {
			const value = rest.join('=');
			return value === '' ? null : value;
		}
	}
	return null;
}

/** A 303 answer to a site path, with cookies applied. */
export function redirect(path: string, apply?: (response: Response) => void): Response {
	const response = new Response(null, { status: 303, headers: { location: path } });
	apply?.(response);
	return response;
}
