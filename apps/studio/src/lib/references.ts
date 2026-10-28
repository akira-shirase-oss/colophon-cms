// Links between entries in plain words (D-065). The server refuses a release that would publish an
// entry linking to an unpublished one (409 `references_unpublished`) or take down an entry a
// published one links to (409 `still_referenced`), without saying which: these work it out from
// the publish request's own entries, so the refusal can name them and say what to do.

import { ApiError, type Changeset, type ContentType, getChangeset, getEntry, getRevision, listContentTypes } from './api.ts';
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
