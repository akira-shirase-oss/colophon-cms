// Addresses in the browser and while prerendering. The docs are served under a base path (`/docs`,
// see lib/markdown/addresses.ts), and the website is the same host's root.

/** The docs' base path, without a trailing slash. */
export const BASE: string = __BASE__;

/** A docs address (`/concepts/`) as the browser needs it (`/docs/concepts/`). */
export function href(path: string): string {
	return path.startsWith('/') && !path.startsWith('//') ? `${BASE}${path}` : path;
}

/** A page's address inside the docs, from the browser's: `/docs/concepts/` is `/concepts/`. */
export function within(pathname: string): string {
	if (BASE && pathname.startsWith(BASE)) return pathname.slice(BASE.length) || '/';
	return pathname;
}

/** An address on the website (the same host's root), for links out of the docs. */
export function site(path: string): string {
	return path;
}
