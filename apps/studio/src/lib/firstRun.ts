// The first-run checklist's two remembered facts (step 3.7, D-059 rule 4). Everything else on
// the list is derived from the project's real state; these two can't be: whether this person
// hid the list, and whether they have looked at a receipt. Both live in this browser only, so
// hiding the list on one device doesn't hide it on another. Storage can be missing or refuse
// (private windows); then the list shows, and "seen" waits for the next visit.

const HIDDEN = 'colophon:first-run-hidden';
const SEEN = 'colophon:receipt-seen';

function read(key: string): boolean {
	try {
		return localStorage.getItem(key) === '1';
	} catch {
		return false;
	}
}

function write(key: string) {
	try {
		localStorage.setItem(key, '1');
	} catch {
		// Not kept: the list shows again next time, which is the honest fallback.
	}
}

/** Whether `identity` hid the checklist of `project`. */
export const isHidden = (identity: string, project: string) => read(`${HIDDEN}:${identity}:${project}`);
export const hide = (identity: string, project: string) => write(`${HIDDEN}:${identity}:${project}`);

/** Whether a receipt of `project` has been opened in this browser. */
export const receiptSeen = (project: string) => read(`${SEEN}:${project}`);
export const markReceiptSeen = (project: string) => write(`${SEEN}:${project}`);

/** "Articles" → "article", "Pages" → "page": what one entry of a type is called. */
export function oneOf(typeName: string): string {
	const lower = typeName.trim().toLowerCase();
	return lower.endsWith('s') && !lower.endsWith('ss') ? lower.slice(0, -1) : lower;
}
