// One-click publishing (D-059, rule 2), shared by the editor and the collection table: make a
// publish request, send it for review, and, when the role may approve and publish its own request
// in the project (`self_review`, D-044), approve it as a recorded self-review and publish it on the
// release the page has seen (D-045). Every step is the ordinary API call with its ordinary checks;
// a refusal stops it and says where things stand, with the request when one was made. It never
// retries past a rule (another release first, a changed entry).

import { ApiError, changesetAction, createChangeset, listReleases, may, newKey, type Project, publishChangeset } from './api.ts';
import { plainError } from './errors.ts';
import { explainLinks } from './references.ts';
import { PUBLISH_REQUEST, VALUE_TAKEN_ON_PUBLISH } from './words.ts';

export type Outcome =
	| { kind: 'published'; generation: number; request: string }
	| { kind: 'unpublished'; generation: number; request: string }
	| { kind: 'requested'; request: string; unpublish: boolean }
	| { kind: 'stopped'; message: string; request: string | null };

/** Whether one click publishes in `collections`, rather than asking for review. */
export function publishesInOneClick(mine: Project | undefined, collections: string[]): boolean {
	return (
		mine?.self_review === true &&
		collections.every((collection) => may(mine, 'content.publish', collection) && may(mine, 'content.review', collection))
	);
}

/** The keys that make pressing again after a lost answer safe: one for the request, one for
 *  publishing it. Make new ones after each attempt. */
export interface Keys {
	request: string;
	publish: string;
}

export const newKeys = (): Keys => ({ request: newKey(), publish: newKey() });

export async function publishInOneClick(options: {
	project: string;
	title: string;
	/** Entries whose current version to publish. */
	entries: string[];
	/** Entries whose live version to take down. */
	unpublish?: string[];
	oneClick: boolean;
	/** The newest release the page has seen: publishing goes on top of it, never past one unseen. */
	seenGeneration: number | undefined;
	keys: Keys;
	/** What is happening now, for the button. */
	step: (words: string) => void;
	/** What the role tried, after "Your role can't", if it is refused. */
	doing?: string;
}): Promise<Outcome> {
	const { project, title, entries, unpublish = [], oneClick, keys, step } = options;
	const takingDown = unpublish.length > 0 && entries.length === 0;
	let request: string | null = null;
	try {
		step(takingDown ? 'Taking it down…' : `Making a ${PUBLISH_REQUEST}…`);
		let made = await createChangeset(project, title, entries, unpublish, keys.request);
		request = made.id;
		if (made.status.state === 'open') {
			step('Sending it for review…');
			made = await changesetAction(project, made.id, made.hash, 'submit');
		}
		if (!oneClick) return { kind: 'requested', request: made.id, unpublish: takingDown };
		if (made.status.state === 'awaiting_review') {
			step('Approving it…');
			made = await changesetAction(project, made.id, made.hash, 'reviews', {
				decision: 'approve',
				comment: 'Approved and published in one step.'
			});
		}
		step('Publishing…');
		const base = options.seenGeneration ?? (await listReleases(project)).generation;
		const published = await publishChangeset(project, made.id, made.hash, base, keys.publish);
		return { kind: takingDown ? 'unpublished' : 'published', generation: published.generation, request: made.id };
	} catch (failure) {
		const reason = failure instanceof ApiError ? failure.reason : null;
		// A refusal about links names the entries, from the request's own (D-065).
		const links = request === null ? null : await explainLinks(project, request, reason);
		if (links !== null) return { kind: 'stopped', message: links.text, request };
		const message =
			reason === 'generation_moved'
				? 'Another release went live while you were publishing. Your request is approved and waiting: look at what is live now, then publish it from the request.'
				: reason === 'stale_changeset'
					? 'An entry changed while it was being published. Open the request to update it to the latest version.'
					: reason === 'value_taken'
						? VALUE_TAKEN_ON_PUBLISH
						: plainError(failure, options.doing ?? (oneClick ? 'publish this' : 'ask for review here'));
		return { kind: 'stopped', message, request };
	}
}
