// How this site reads Colophon (D-220). The website's Next.js guide quotes this file, so it uses plain `fetch`.
//
// The rule it keeps: one build, one release. Next.js renders pages in several worker processes, so the release
// can't be learned in memory the way a single-process build could: scripts/pin-release.mjs learns it once, before
// `next build`, and writes .release.json; every worker reads that file, and every read asks for that release with
// `?generation=N`. An answer naming any other release fails the build.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';

/** Where the site reads: the API origin, the project's id and the environment. */
export interface Site {
	api: string;
	project: string;
	environment: string;
}

/** A recipe as the delivery API answers it. */
export interface Recipe {
	id: string;
	published_in: number;
	content: {
		title: string;
		slug?: string;
		intro?: string;
		category?: string;
		difficulty?: string;
		prep_minutes?: number;
		cook_minutes?: number;
		servings?: number;
		ingredients?: { item: string; amount?: string }[];
		steps?: string[];
	};
}

interface Page {
	data?: Recipe[];
	next_cursor?: string | null;
}

const PAGE_SIZE = 100;
/** More pages than this means something is wrong, so the build stops instead of looping. */
const MAX_PAGES = 50;
/** Every request has a deadline: a CMS that doesn't answer fails the build, it doesn't hang it. */
const TIMEOUT_MS = 10_000;

/** The release this build reads, as scripts/pin-release.mjs wrote it, or null when nothing is seeded. */
export function buildRelease(): { site: Site; generation: number } | null {
	try {
		return JSON.parse(readFileSync(join(process.cwd(), '.release.json'), 'utf8'));
	} catch {
		return null;
	}
}

/** Every published entry of `collection`, read at release `generation`, page by page. */
export async function entriesAt(site: Site, collection: string, generation: number): Promise<Recipe[]> {
	const items: Recipe[] = [];
	let cursor: string | null = null;
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const url = new URL(`${site.api}/content/v1/${site.project}/${site.environment}/${collection}`);
		url.searchParams.set('generation', String(generation));
		url.searchParams.set('limit', String(PAGE_SIZE));
		if (cursor !== null) url.searchParams.set('cursor', cursor);
		const response = await fetch(url, { headers: { accept: 'application/json' }, signal: AbortSignal.timeout(TIMEOUT_MS) });
		if (!response.ok) throw new Error(`GET ${url.pathname}${url.search}: HTTP ${response.status}`);
		const answered = Number(response.headers.get('colophon-generation'));
		if (answered !== generation) throw new Error(`asked for release ${generation}, got ${answered}`);
		const body = (await response.json()) as Page;
		items.push(...(body.data ?? []));
		cursor = body.next_cursor ?? null;
		if (cursor === null) return items;
	}
	throw new Error(`more than ${MAX_PAGES} pages in ${collection}`);
}
