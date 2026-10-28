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

/** A text field's pattern (D-069): the form its values must take, said as such. */
export const ALLOWED_FORM = 'allowed form';
/** The server refused a pattern it can't use (`invalid_pattern`). */
export const INVALID_PATTERN =
	'Colophon can’t use this allowed form. Write it as a regular expression, like [A-Z]{3}-[0-9]{4}, without look-around or back-references.';
/** A value the server found not in its field's allowed form (`pattern_mismatch`), with the field's
 *  help text, which is where a type says what the form is. */
export function patternMismatch(label: string, help = ''): string {
	return `${label} isn't in its ${ALLOWED_FORM}.${help.trim() === '' ? '' : ` ${help.trim()}`}`;
}

/** A field's default (D-071), as the builder offers it, and its refusal (`invalid_default`) when the
 *  server's own words can't be shown. */
export const DEFAULT_VALUE = 'Default value (optional)';
export const INVALID_DEFAULT = 'This default isn’t a value the field accepts. Check its choices, limits and allowed form, or clear it.';

/** Rate limits (D-076): the server turned a request away for a moment because this person or
 *  address sent many at once, and its `Retry-After` said how long to wait. `auto`: the Studio
 *  sends it again by itself (a save, a read); otherwise the person does. */
export function busyFor(seconds: number, auto: boolean): string {
	const count = Math.max(1, Math.ceil(seconds));
	const wait = `${count} second${count === 1 ? '' : 's'}`;
	return `Colophon is busy for a moment; ${auto ? 'trying' : 'try'} again in ${wait}.`;
}

/** Shared components (D-073): their two refusals. */
export const UNKNOWN_COMPONENT = 'That component doesn’t exist in this project. Choose one from the list.';
export const INVALID_COMPONENT = 'A component can’t hold another component, a URL name, or fields that are unique, have a default, or show only sometimes.';

/** Conditional fields (D-072): the builder's control, its refusal (`invalid_condition`), and what
 *  the editor says when a field stops applying and its value is cleared. */
export const SHOW_WHEN = 'Show only when';
export const INVALID_CONDITION =
	'This condition can’t be used: it must name a yes-or-no or choice field that always applies, with some of its values, and the title always shows.';
export function clearedNote(label: string, dependsOn: string, values: string[]): string {
	return `${label} is cleared because it only applies when ${dependsOn} is ${values.join(' or ')}.`;
}

/** A unique field (D-070), as the builder offers it. */
export const UNIQUE = 'No two entries may share this';
/** A value another entry already has (`value_taken` on a save), said under the field. */
export function valueTaken(label: string): string {
	return `Another entry already has this ${label}.`;
}
/** A release that would publish a value another published entry still has (`value_taken`). */
export const VALUE_TAKEN_ON_PUBLISH =
	'A value in it that must be unique is still what readers get from another entry. Change that entry, or take it down, first or in the same publish request.';
/** A field that can't be unique (`invalid_unique`). */
export const INVALID_UNIQUE =
	'Only a one-line text, email address, web address, number, date, time, or date and time at the top of the type can be kept unique.';

/** "Release 4, live since 3 minutes ago" pieces: a release is named by its number. */
export function releaseName(generation: number): string {
	return `Release ${generation}`;
}

/** Internal terms that must not show outside Settings and the Technical details drawer. */
export const INTERNAL_TERMS =
	/\b(changesets?|revisions?|slugs?|destinations?|webhooks?|probes?|generations?|pinned|pins?|hash(es)?|etags?|idempotency|endpoints?|payloads?)\b/i;
