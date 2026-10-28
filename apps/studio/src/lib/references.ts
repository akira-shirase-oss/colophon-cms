// Links between entries in plain words (D-065). The server refuses a release that would publish an
// entry linking to an unpublished one (409 `references_unpublished`) or take down an entry a
// published one links to (409 `still_referenced`), without saying which: these work it out from
// the publish request's own entries, so the refusal can name them and say what to do.

import { ApiError, type Changeset, type Content, type ContentType, type FieldDefinition, getChangeset, getEntry, getRevision, listContentTypes } from './api.ts';
import { blockOf, isRecord } from './content.ts';
import { PUBLISH_REQUEST, shownTitle } from './words.ts';

/** An entry named in an explanation: its title, or "an entry you can't see". */
export interface Named {
	id: string;
	collection: string;
	title: string;
}

async function titled(project: string, collection: string, id: string): Promise<{ named: Named; published: boolean | null }> {
	try {
		const { draft } = await getEntry(project, collection, id);
		return { named: { id, collection, title: `“${shownTitle(draft.revision.title)}”` }, published: draft.published === undefined ? null : draft.published !== null };
	} catch (failure) {
		if (failure instanceof ApiError && (failure.failure === 'not-found' || failure.status === 403)) {
			return { named: { id, collection, title: 'an entry you can’t see' }, published: null };
		}
		throw failure;
	}
}

/** One linked entry's title, read on its own and shared by the picker and "Linked from": a refused
 *  read is an entry the role can't see (`seen: false`), never shown by its id. */
export const linkedQuery = (project: string, collection: string, id: string) => ({
	queryKey: ['linked', project, collection, id],
	queryFn: async (): Promise<{ title: string; seen: boolean; content?: Content }> => {
		try {
			const { draft } = await getEntry(project, collection, id);
			return { title: draft.revision.title, seen: true, content: draft.content };
		} catch (failure) {
			if (failure instanceof ApiError && (failure.failure === 'not-found' || failure.status === 403)) return { title: '', seen: false };
			throw failure;
		}
	},
	staleTime: 30_000
});

/**
 * A place in an entry in words: "Author", or inside structures "Sections → Quote 2 → By", from a
 * path like `sections/1/by`. A page section's kind comes from the entry's content; without it the
 * section is just "Section 2". Empty when the path names no field.
 */
export function placeName(fields: FieldDefinition[], path: string, content?: Content): string {
	const names: string[] = [];
	let here: FieldDefinition[] = fields;
	let value: unknown = content;
	const segments = path.split('/');
	for (let at = 0; at < segments.length; at += 1) {
		const field = here.find((candidate) => candidate.id === segments[at]);
		if (field === undefined) break;
		names.push(field.label);
		const inside = isRecord(value) ? value[field.id] : undefined;
		if (field.type === 'object') {
			here = field.fields;
			value = inside;
		} else if (field.type === 'list' || field.type === 'blocks') {
			const index = Number(segments[at + 1]);
			if (!Number.isInteger(index)) break;
			at += 1;
			const item = Array.isArray(inside) ? inside[index] : undefined;
			if (field.type === 'blocks') {
				const kind = isRecord(item) ? blockOf(field, item) : undefined;
				names.push(`${kind?.label ?? 'Section'} ${index + 1}`);
				here = kind?.fields ?? [];
			} else {
				names.push(`Item ${index + 1}`);
				here = field.of.type === 'object' ? field.of.fields : [];
			}
			value = item;
		} else {
			break;
		}
	}
	return names.join(' → ');
}

const list = (names: string[]) => (names.length <= 1 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`);

/** Entries a request's items link to that aren't published and aren't in it: what blocks it. */
export async function unpublishedLinks(project: string, set: Changeset, types: ContentType[]): Promise<{ from: string; to: Named }[]> {
	const publishing = set.items.filter((item) => item.kind === 'publish');
	const inRequest = new Set(publishing.map((item) => item.entry_id));
	const found: { from: string; to: Named }[] = [];
	const seen = new Set<string>();
	for (const item of publishing) {
		const definition = types.find((type) => type.slug === item.collection)?.definition;
		if (definition === undefined) continue;
		const links = definition.fields.filter((field) => field.type === 'reference');
		if (links.length === 0) continue;
		const { content } = await getRevision(project, item.collection, item.entry_id, item.revision_id);
		for (const field of links) {
			if (field.type !== 'reference') continue;
			const value = content[field.id];
			const ids = Array.isArray(value) ? value : typeof value === 'string' ? [value] : [];
			for (const id of ids) {
				if (typeof id !== 'string' || inRequest.has(id) || seen.has(id)) continue;
				const { named, published } = await titled(project, field.to, id);
				// Published entries don't block; ones the role can't see may, so they are named too.
				if (published === true) continue;
				seen.add(id);
				found.push({ from: `“${shownTitle(item.title)}”`, to: named });
			}
		}
	}
	return found;
}

/** Published entries that still link to an entry the request takes down. */
export async function publishedReferrers(project: string, set: Changeset): Promise<{ to: string; from: Named }[]> {
	const leaving = set.items.filter((item) => item.kind === 'unpublish');
	const alsoLeaving = new Set(leaving.map((item) => item.entry_id));
	const found: { to: string; from: Named }[] = [];
	for (const item of leaving) {
		const { draft } = await getEntry(project, item.collection, item.entry_id);
		for (const referrer of draft.referenced_by ?? []) {
			if (!referrer.published || alsoLeaving.has(referrer.entry_id)) continue;
			const { named } = await titled(project, referrer.collection, referrer.entry_id);
			found.push({ to: `“${shownTitle(item.title)}”`, from: named });
		}
	}
	return found;
}

/**
 * A refusal about links, said with names when the request's entries tell them; `null` when it
 * isn't one, or the names can't be found (the general words in lib/errors.ts then apply).
 */
export async function explainLinks(
	project: string,
	request: string,
	reason: string | null,
	types?: ContentType[]
): Promise<{ text: string; missing: Named[] } | null> {
	if (reason !== 'references_unpublished' && reason !== 'still_referenced') return null;
	try {
		const set = await getChangeset(project, request);
		if (reason === 'references_unpublished') {
			const links = await unpublishedLinks(project, set, types ?? (await listContentTypes(project)));
			if (links.length === 0) return null;
			const lines = links.map((link) => `${link.from} links to ${link.to.title}, which isn't published yet.`);
			const them = links.length === 1 ? links[0]?.to.title ?? 'it' : 'them';
			return {
				text: `${lines.join(' ')} Publish ${them} first, or in the same ${PUBLISH_REQUEST}.`,
				missing: links.map((link) => link.to)
			};
		}
		const referrers = await publishedReferrers(project, set);
		if (referrers.length === 0) return null;
		const lines = referrers.map((one) => `${one.to} is still linked from ${one.from.title}, which readers have.`);
		return {
			text: `${lines.join(' ')} Change or take down ${list([...new Set(referrers.map((one) => one.from.title))])} first, or in the same ${PUBLISH_REQUEST}.`,
			missing: []
		};
	} catch {
		return null;
	}
}
