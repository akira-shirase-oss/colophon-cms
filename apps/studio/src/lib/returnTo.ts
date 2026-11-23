import { goto } from '$app/navigation';
import { resolve } from '$app/paths';

/** Goes to a same-origin `return_to` after signing in. A Studio page is a client-side move; any
 *  other path on this origin (the server's `/oauth/authorize`, D-246) is a full page load, which
 *  the Studio's router can't do. Callers check `sameOrigin` first. */
export async function followReturn(path: string, replace = false): Promise<void> {
	const pathname = new URL(path, window.location.origin).pathname;
	const base = resolve('/').replace(/\/$/, '');
	if (pathname === base || pathname.startsWith(`${base}/`)) {
		await goto(path, { replaceState: replace });
	} else if (replace) {
		window.location.replace(path);
	} else {
		window.location.assign(path);
	}
}
