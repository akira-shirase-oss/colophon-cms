/** Studio v2 proposal: which direction to draw, `a` (website-faithful) or `b` (bolder), as `data-direction` on
 *  <html>. `?direction=a|b` picks one and remembers it in this browser; anything else keeps the last choice, and
 *  a first visit gets A. Proposal-only: it goes once the maintainer picks. */
const KEY = 'colophon-studio-direction';

export function applyDirection(search: string): void {
	let chosen: string | null = new URLSearchParams(search).get('direction');
	try {
		if (chosen === 'a' || chosen === 'b') localStorage.setItem(KEY, chosen);
		else chosen = localStorage.getItem(KEY);
	} catch {
		// storage off: the URL alone decides
	}
	document.documentElement.dataset.direction = chosen === 'b' ? 'b' : 'a';
}
