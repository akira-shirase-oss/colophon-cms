// What went wrong, in the Studio's words (D-059, rule 3): every failure says what happened and
// what to do next, without the API's internal names. The server's own message stays available for
// the Technical details drawer, where developers and support look.
//
// Refusals are recognised by their status and `details.reason`, a stable code the server gives
// every 403 and 409 (crates/colophon-api/src/error.rs); a 403 for a missing permission also names
// the permission (`details.permission`). The server's wording is never matched.

import { type Action, ApiError } from './api.ts';
import { INTERNAL_TERMS, PUBLISH_REQUEST, URL_NAME, WEBSITE_CONNECTIONS } from './words.ts';

/** A failure, said plainly: a short title, what happened and what to do, and the server's words. */
export interface Said {
	title: string;
	text: string;
	/** The server's message, status and reason, for the Technical details drawer. */
	technical: string;
}

/** Known refusals by `details.reason`. */
const BY_REASON: Record<string, string> = {
	// 409: the state moved on, or the request clashes with it.
	stale_changeset: `This ${PUBLISH_REQUEST} changed since you loaded it. Look at the current one, then try again.`,
	generation_moved: 'Another release went live first. Look at what is live now, then try again.',
	stale_revision: 'Someone else saved this entry first. Reload it to see their version.',
	stale_version: 'Someone changed this while you were editing it. Load the latest version, then make your change again.',
	stale_role: 'Someone changed this role while you were editing it. Load the latest version, then make your change again.',
	role_in_use: 'People still hold this role, or invitations offer it. Give them another role first.',
	slug_taken: `Another entry already uses that ${URL_NAME}. Choose a different one, or publish the other entry's new ${URL_NAME} first.`,
	already_published: `This ${PUBLISH_REQUEST} is already published.`,
	not_approved: `Only an approved ${PUBLISH_REQUEST} can be published. Ask for a review first.`,
	not_submitted: "It hasn't been sent for review yet. Press Ask for review first.",
	entries_moved_on: `An entry in it was edited after the ${PUBLISH_REQUEST} was made. Update it to the latest versions, then review it again.`,
	approval_expired: 'The approval has expired. It needs approving again before it can be published.',
	already_scheduled: 'It is already scheduled. Cancel that schedule to choose another time.',
	schedule_published: 'That schedule has published already; cancelling never undoes a release.',
	schedule_blocked: 'That schedule was blocked when its time came, so there is nothing to cancel.',
	schedule_cancelled: 'That schedule was cancelled already.',
	last_owner: 'A project must keep at least one owner. Make someone else an owner first.',
	already_member: "You're already a member of this project.",
	owner_role_fixed: 'The Owner role can do everything and never changes.',
	role_exists: 'A role with that short name exists already. Choose another.',
	project_exists: 'A project with that URL name exists already. Choose another.',
	content_type_exists: `A content type with that ${URL_NAME} exists already. Choose another.`,
	already_set_up: 'Colophon is already set up. Reload the page.',
	idempotency_key_reused: 'That was sent twice with different contents. Reload the page and try again.',
	setup_code_invalid: "That code isn't right, or it was used or has expired.",
	// 422s and 409s for links between entries (D-065). The two 409s get names on the pages that
	// know the request's entries (lib/references.ts); these are the words without them.
	reference_not_found: 'A linked entry isn’t there, or isn’t in the collection the field links to. Remove it and choose again.',
	references_unpublished: `An entry in it links to an entry that isn't published yet. Publish that entry first, or in the same ${PUBLISH_REQUEST}.`,
	still_referenced: `A published entry still links to an entry this takes down. Change or take down that entry first, or in the same ${PUBLISH_REQUEST}.`,
	// 422s for API tokens.
	not_for_tokens: 'An API token can’t manage people, roles, tokens or the activity log.',
	invalid_expiry: 'A token lasts between 1 and 366 days.',
	permissions_required: 'Choose at least one thing a custom token may do.',
	permissions_not_applicable: 'Only a custom token has its own list of what it may do.',
	too_many_tokens: 'A project has at most 100 active tokens. Revoke one you no longer use first.',
	invalid_token_name: 'A token’s name is 1 to 100 characters.',
	invalid_token_description: 'A token’s description is at most 500 characters.',
	// 403s that aren't about what the role may do.
	cannot_grant: "That would give powers your own role doesn't have.",
	api_token_not_allowed: 'An API token can’t do this; a person has to.',
	administrators_only: 'Only the people who run this Colophon can do this.',
	not_set_up: 'Colophon isn’t set up yet. Its administrator finishes that first.',
	browser_session_required: 'This needs you signed in here, in the browser. Reload the page and sign in.',
	origin_not_allowed: 'This page is out of date. Reload it and try again.',
	csrf_token_invalid: 'This page is out of date. Reload it and try again.'
};

/** What each permission lets someone do, after "Your role can't". */
const DOES: Record<Action, string> = {
	'content.read': 'see this content',
	'content.create': 'create entries here',
	'content.update': 'change this content',
	'content.delete': 'delete entries here',
	'content.review': 'approve or ask for changes here',
	'content.publish': 'publish this content',
	'schema.change': 'change content types',
	'members.manage': 'manage members',
	'roles.manage': 'change roles',
	'destinations.manage': `manage ${WEBSITE_CONNECTIONS}`,
	'tokens.manage': 'manage API tokens',
	'audit.read': 'see the activity log'
};

/** The server's message as a sentence, if it has no internal words; else `fallback`. */
function plainOr(message: string, fallback: string): string {
	if (message === '' || INTERNAL_TERMS.test(message)) return fallback;
	// The server quotes labels in backticks (`Question` is required); people read them plainly.
	const words = message.replace(/`([^`]+)`/g, '$1');
	const sentence = words.slice(0, 1).toUpperCase() + words.slice(1);
	return /[.!?]$/.test(sentence) ? sentence : `${sentence}.`;
}

/**
 * `failure`, said plainly. `doing` names what the person tried, as a verb phrase that follows
 * "Your role can't": "approve this", "ask for review here"; without it, the permission the server
 * names says it.
 */
export function describe(failure: unknown, doing?: string): Said {
	if (!(failure instanceof ApiError)) {
		const message = failure instanceof Error ? failure.message : String(failure);
		return {
			title: 'Something went wrong',
			text: 'Something unexpected happened. Reload the page and try again; if it keeps happening, tell whoever runs Colophon.',
			technical: message
		};
	}
	const technical = [failure.status === null ? 'no answer' : `HTTP ${failure.status}`, failure.reason, failure.message]
		.filter((part) => part !== null && part !== '')
		.join(' · ');
	const known = failure.reason === null ? undefined : BY_REASON[failure.reason];
	const permission = failure.details?.permission as Action | undefined;
	const action = doing ?? (permission === undefined ? undefined : DOES[permission]) ?? 'do this';
	switch (failure.failure) {
		case 'signed-out':
			return { title: "You're signed out", text: 'Sign in again, then try once more. Edits you made here are kept on this device.', technical };
		case 'not-found':
			return { title: 'Not found', text: "It isn't there any more, or your role no longer lets you see it. Reload the page.", technical };
		case 'retry':
			return failure.status === 429
				? { title: 'Too many tries', text: plainOr(failure.message, 'Wait a minute, then try again.'), technical }
				: failure.status === null
					? { title: "Colophon didn't answer", text: 'Check your connection, then try again.', technical }
					: { title: 'Colophon had a problem', text: 'Try again in a moment; if it keeps happening, tell whoever runs Colophon.', technical };
		case 'conflict':
			return { title: 'Changed meanwhile', text: known ?? 'Someone changed this meanwhile. Reload the page and try again.', technical };
		case 'refused':
			if (failure.status === 403) {
				return known !== undefined
					? { title: 'Not allowed', text: known, technical }
					: { title: "Your role can't do this", text: `Your role in this project can't ${action}. If you need to, ask one of the project's owners.`, technical };
			}
			if (failure.status === 409) {
				return { title: 'Not done', text: known ?? plainOr(failure.message, 'Something changed meanwhile. Reload the page and try again.'), technical };
			}
			return {
				title: 'Not accepted',
				text: known ?? plainOr(failure.message, 'Colophon couldn’t accept this. Check what you entered and try again.'),
				technical
			};
	}
}

/** Just the words to show inline: what happened and what to do. */
export function plainError(failure: unknown, doing?: string): string {
	return describe(failure, doing).text;
}
