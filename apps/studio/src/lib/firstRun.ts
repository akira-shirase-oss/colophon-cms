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

/**
 * Where a first entry goes (D-082): the first collection the person made, never a one-entry type
 * (Homepage), else the built-in Articles, else any collection. "First made" is read from each
 * type's `created_at`, which is its current version's: a collection changed after a later one was
 * made can lose its place. The API doesn't say when a type was first made.
 */
export function mainCollection<T extends { slug: string; created_at: string; definition: { single?: boolean } }>(types: T[]): T | undefined {
	const collections = types.filter((type) => type.definition.single !== true);
	const made = collections.filter((type) => type.slug !== 'articles').sort((a, b) => a.created_at.localeCompare(b.created_at));
	return made[0] ?? collections.find((type) => type.slug === 'articles') ?? collections[0];
}

/** "Articles" → "article", "Pages" → "page": what one entry of a type is called. */
export function oneOf(typeName: string): string {
	const lower = typeName.trim().toLowerCase();
	return lower.endsWith('s') && !lower.endsWith('ss') ? lower.slice(0, -1) : lower;
}
