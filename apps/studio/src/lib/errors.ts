// What went wrong, in the Studio's words (D-059, rule 3): every failure says what happened and
// what to do next, without the API's internal names. The server's own message stays available for
// the Technical details drawer, where developers and support look.
//
// Refusals are recognised by their status and `details.reason`. A few 409s carry no reason yet
// and are recognised by their message; when the server gives every refusal a stable reason (B3),
// match those reasons here and drop the message patterns.

import { ApiError } from './api.ts';
import { INTERNAL_TERMS, PUBLISH_REQUEST, URL_NAME } from './words.ts';

/** A failure, said plainly: a short title, what happened and what to do, and the server's words. */
export interface Said {
	title: string;
	text: string;
	/** The server's message, status and reason, for the Technical details drawer. */
	technical: string;
}

/** Known refusals by `details.reason`. */
const BY_REASON: Record<string, string> = {
	stale_changeset: `This ${PUBLISH_REQUEST} changed since you loaded it. Look at the current one, then try again.`,
	generation_moved: 'Another release went live first. Look at what is live now, then try again.',
	stale_revision: 'Someone else saved this entry first. Reload it to see their version.',
	stale_version: 'Someone changed this while you were editing it. Load the latest version, then make your change again.',
	stale_role: 'Someone changed this role while you were editing it. Load the latest version, then make your change again.',
	role_in_use: 'People still hold this role, or invitations offer it. Give them another role first.',
	slug_taken: `Another entry already uses that ${URL_NAME}. Choose a different one.`,
	already_set_up: 'Colophon is already set up. Reload the page.',
	idempotency_key_reused: 'That was sent twice with different contents. Reload the page and try again.',
	setup_code_invalid: "That code isn't right, or it was used or has expired."
};

/** 409s without a reason yet (B3), by what the server says. Each pattern is a phrase the server
 *  writes today (crates/colophon-api/src/changesets.rs). */
const BY_MESSAGE: [RegExp, string][] = [
	[/hasn't been submitted for review/i, `It hasn't been sent for review yet. Press Ask for review first.`],
	[
		/moved on after this changeset pinned it/i,
		`An entry in it was edited after the ${PUBLISH_REQUEST} was made. Update it to the latest versions, then review it again.`
	],
	[/already been published/i, `This ${PUBLISH_REQUEST} is already published.`],
	[/approval has expired/i, 'The approval has expired. It needs approving again before it can be published.'],
	[/only an approved changeset/i, `Only an approved ${PUBLISH_REQUEST} can be published. Ask for a review first.`],
	[/slug that another entry still publishes/i, `Another live entry still uses this ${URL_NAME}. Publish that entry's new ${URL_NAME} first, or in the same ${PUBLISH_REQUEST}.`],
	[/CSRF|origin may not make changes/i, 'This page is out of date. Reload it and try again.']
];

/** The server's message as a sentence, if it has no internal words; else `fallback`. */
function plainOr(message: string, fallback: string): string {
	if (message === '' || INTERNAL_TERMS.test(message)) return fallback;
	const sentence = message.slice(0, 1).toUpperCase() + message.slice(1);
	return /[.!?]$/.test(sentence) ? sentence : `${sentence}.`;
}

/**
 * `failure`, said plainly. `doing` names what the person tried, as a verb phrase that follows
 * "Your role can't": "approve this", "publish this", "see this".
 */
export function describe(failure: unknown, doing = 'do this'): Said {
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
	const matched = BY_MESSAGE.find(([pattern]) => pattern.test(failure.message))?.[1];
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
			return { title: 'Changed meanwhile', text: known ?? matched ?? 'Someone changed this meanwhile. Reload the page and try again.', technical };
		case 'refused':
			if (failure.status === 403) {
				return matched !== undefined
					? { title: 'Out of date', text: matched, technical }
					: { title: "Your role can't do this", text: `Your role in this project can't ${doing}. If you need to, ask one of the project's owners.`, technical };
			}
			if (failure.status === 409) {
				return { title: 'Not done', text: known ?? matched ?? plainOr(failure.message, 'Something changed meanwhile. Reload the page and try again.'), technical };
			}
			return {
				title: 'Not accepted',
				text: known ?? matched ?? plainOr(failure.message, 'Colophon couldn’t accept this. Check what you entered and try again.'),
				technical
			};
	}
}

/** Just the words to show inline: what happened and what to do. */
export function plainError(failure: unknown, doing?: string): string {
	return describe(failure, doing).text;
}
