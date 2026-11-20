// A publish request's one next step (the Studio UI pass, workstream 3): what the person looking at
// it can do now, given where it stands (D-044's states, a pending schedule from D-060) and what
// their role allows. Each step has at most one primary button, so the page never offers three
// equal "next" actions. Pure: the page renders the step, and tests/request-state.spec.ts checks
// every combination.

import type { ChangesetState } from './api.ts';

/** What the person may do with this request. */
export interface Viewer {
	/** May send it for review and update it: edit rights on every entry in it. */
	writer: boolean;
	/** May approve it or ask for changes: review rights on every collection in it, and not their
	 *  own request unless the project lets people approve their own (D-044). */
	decides: boolean;
	/** May publish it, and so schedule it: publish rights on every collection in it. */
	publisher: boolean;
	/** May cancel its pending schedule: who set it, or a publisher. */
	cancels: boolean;
}

export type NextStep =
	/** Prepared: send it for review. */
	| { kind: 'ask-review' }
	| { kind: 'cannot-send' }
	/** An entry changed after it was added: pin the latest versions. */
	| { kind: 'update' }
	| { kind: 'cannot-update' }
	/** Waiting for a decision this person may make: approve, or ask for changes. */
	| { kind: 'decide' }
	/** Waiting for someone else's decision. */
	| { kind: 'waiting' }
	/** A reviewer asked for changes: edit the entries, then update it. */
	| { kind: 'changes-asked' }
	/** Approved: publish it now, or schedule it. */
	| { kind: 'publish' }
	/** Approved, for someone who may publish it. */
	| { kind: 'approved' }
	/** Approved and scheduled: it publishes by itself; it can still go now, or be called off. */
	| { kind: 'scheduled' }
	/** Live: nothing left to do here. */
	| { kind: 'published' };

export function nextStep(state: ChangesetState, scheduled: boolean, viewer: Viewer): NextStep {
	switch (state) {
		case 'open':
			return { kind: viewer.writer ? 'ask-review' : 'cannot-send' };
		case 'stale':
			return { kind: viewer.writer ? 'update' : 'cannot-update' };
		case 'awaiting_review':
		case 'approval_expired':
			return { kind: viewer.decides ? 'decide' : 'waiting' };
		case 'changes_requested':
			return { kind: viewer.decides ? 'decide' : 'changes-asked' };
		case 'approved':
			if (scheduled) return { kind: 'scheduled' };
			return { kind: viewer.publisher ? 'publish' : 'approved' };
		case 'published':
			return { kind: 'published' };
	}
}

/** The buttons a step shows, by name: the one primary (if any) and the secondary ones. */
export interface StepActions {
	primary: string | null;
	secondary: string[];
}

export const ACTION = {
	askReview: 'Ask for review',
	update: 'Update to the latest versions',
	approve: 'Approve',
	requestChanges: 'Request changes',
	publishNow: 'Publish now',
	schedule: 'Schedule…',
	cancelSchedule: 'Cancel schedule'
} as const;

export function actionsOf(step: NextStep, viewer: Viewer): StepActions {
	switch (step.kind) {
		case 'ask-review':
			return { primary: ACTION.askReview, secondary: [] };
		case 'update':
			return { primary: ACTION.update, secondary: [] };
		case 'decide':
			return { primary: ACTION.approve, secondary: [ACTION.requestChanges] };
		case 'publish':
			return { primary: ACTION.publishNow, secondary: [ACTION.schedule] };
		case 'scheduled':
			// Both secondary: the schedule is already the plan; these change it.
			return {
				primary: null,
				secondary: [...(viewer.publisher ? [ACTION.publishNow] : []), ...(viewer.cancels ? [ACTION.cancelSchedule] : [])]
			};
		case 'cannot-send':
		case 'cannot-update':
		case 'waiting':
		case 'changes-asked':
		case 'approved':
		case 'published':
			return { primary: null, secondary: [] };
	}
}

/** "Sam", "Sam and Alex", "Sam, Alex and Kim": the people a request waits for. */
export function names(people: string[]): string {
	if (people.length <= 1) return people[0] ?? '';
	return `${people.slice(0, -1).join(', ')} and ${people.at(-1) ?? ''}`;
}

/** The title of the task that asks someone to review a request (at most 200 characters, the
 *  server's limit): `Review “Spring launch”`, then the sender's note when there is one. */
export function reviewTaskTitle(request: string, note: string): string {
	const LIMIT = 200;
	const said = note.trim().replace(/\s+/g, ' ');
	const tail = said === '' ? '' : `: ${said}`;
	const room = Math.max(12, LIMIT - 'Review “”'.length - tail.length);
	const name = request.length > room ? `${request.slice(0, room - 1)}…` : request;
	return `Review “${name}”${tail}`.slice(0, LIMIT);
}
