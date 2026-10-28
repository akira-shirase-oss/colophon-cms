// The Studio's words for Colophon's ideas (D-059, rule 1): the one place they come from, so an
// editor meets one name for each thing on every screen. The API, the CLI and the "Technical
// details" drawer keep the internal names (changeset, revision, slug, destination, generation);
// everywhere else uses these. A test fails on internal terms in visible text outside Settings
// and that drawer (`INTERNAL_TERMS`, tests/server/words.spec.ts).

import type { ChangesetState, Destination } from './api.ts';

/** A changeset: exact versions of entries, asked to be published. */
export const PUBLISH_REQUEST = 'publish request';
export const PUBLISH_REQUESTS = 'publish requests';
/** A revision: one saved state of an entry. */
export const VERSION = 'version';
export const VERSIONS = 'versions';
/** A slug: the part of an address that names an entry. */
export const URL_NAME = 'URL name';
/** Destinations: what each release is announced to. */
export const WEBSITE_CONNECTION = 'website connection';
export const WEBSITE_CONNECTIONS = 'website connections';
/** A release keeps its name; people see its number and when it went live. */
export const RELEASE = 'release';

/** `publish request` → `Publish request`. */
export function capital(text: string): string {
	return text.slice(0, 1).toUpperCase() + text.slice(1);
}

/** "1 publish request", "3 publish requests". */
export function count(n: number, one: string, many: string): string {
	return `${n.toLocaleString()} ${n === 1 ? one : many}`;
}

/** What each kind of website connection does, as a noun and as the action it takes. */
export const CONNECTION_KIND: Record<Destination['kind'], { name: string; does: string; detail: string }> = {
	webhook: { name: 'Site rebuild', does: 'Rebuild the site', detail: 'Tells your website a new release is out, so it can rebuild.' },
	cdn: { name: 'CDN clear', does: 'Clear the CDN', detail: 'Asks your CDN to drop the copies each release changed.' },
	probe: { name: 'Site check', does: 'Check the site', detail: 'Reads your live site after each release to confirm readers see it.' }
};

/** A publish request's state, in words (D-044's states). */
export const REQUEST_STATE: Record<ChangesetState, string> = {
	open: 'Not sent for review',
	stale: 'Edited since: update it',
	awaiting_review: 'Waiting for review',
	changes_requested: 'Changes asked for',
	approved: 'Approved',
	approval_expired: 'Approval expired',
	published: 'Published'
};

/** An entry's title as a list shows it. The server leaves a title out when the role may not read
 *  the title field, and a draft can have none yet; either way the row still needs words. */
export function shownTitle(title: string): string {
	return title.trim() === '' ? 'No title to show' : title;
}

/** "Release 4, live since 3 minutes ago" pieces: a release is named by its number. */
export function releaseName(generation: number): string {
	return `Release ${generation}`;
}

/** Internal terms that must not show outside Settings and the Technical details drawer. */
export const INTERNAL_TERMS =
	/\b(changesets?|revisions?|slugs?|destinations?|webhooks?|probes?|generations?|pinned|pins?|hash(es)?|etags?|idempotency|endpoints?|payloads?)\b/i;
