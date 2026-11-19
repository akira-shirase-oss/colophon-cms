// A publish request's next step (lib/requestState.ts): one primary per state, never three. The
// audit's P1-5 found Publish, Schedule and "Approve this version" all primary on an approved
// request; each state now leads with one thing, and some lead with none.

import { expect, test } from '@playwright/test';
import type { ChangesetState } from '../src/lib/api.ts';
import { ACTION, actionsOf, names, nextStep, reviewTaskTitle, type Viewer } from '../src/lib/requestState.ts';

const STATES: ChangesetState[] = ['open', 'stale', 'awaiting_review', 'changes_requested', 'approved', 'approval_expired', 'published'];
const owner: Viewer = { writer: true, decides: true, publisher: true, cancels: true };
const author: Viewer = { writer: true, decides: false, publisher: false, cancels: false };

test('every state, for every role, has at most one primary', () => {
	for (const state of STATES) {
		for (const scheduled of [false, true]) {
			for (let bits = 0; bits < 16; bits += 1) {
				const viewer: Viewer = { writer: (bits & 1) > 0, decides: (bits & 2) > 0, publisher: (bits & 4) > 0, cancels: (bits & 8) > 0 };
				const actions = actionsOf(nextStep(state, scheduled, viewer), viewer);
				// A primary is one name or none, and never also offered as a secondary.
				expect(actions.secondary, `${state} ${scheduled} ${bits}`).not.toContain(actions.primary);
			}
		}
	}
});

test('each state leads with its one next step', () => {
	const lead = (state: ChangesetState, viewer: Viewer, scheduled = false) => actionsOf(nextStep(state, scheduled, viewer), viewer);
	// Prepared: ask for review.
	expect(lead('open', author)).toEqual({ primary: ACTION.askReview, secondary: [] });
	// Waiting: the reviewer decides; the author waits, with nothing to press.
	expect(lead('awaiting_review', owner)).toEqual({ primary: ACTION.approve, secondary: [ACTION.requestChanges] });
	expect(nextStep('awaiting_review', false, author)).toEqual({ kind: 'waiting' });
	expect(lead('awaiting_review', author)).toEqual({ primary: null, secondary: [] });
	// Approved: publish now, or schedule it.
	expect(lead('approved', owner)).toEqual({ primary: ACTION.publishNow, secondary: [ACTION.schedule] });
	expect(lead('approved', author)).toEqual({ primary: null, secondary: [] });
	// Scheduled: the time leads; publishing now and cancelling are both secondary.
	expect(lead('approved', owner, true)).toEqual({ primary: null, secondary: [ACTION.publishNow, ACTION.cancelSchedule] });
	expect(lead('approved', { ...author, cancels: true }, true)).toEqual({ primary: null, secondary: [ACTION.cancelSchedule] });
	// Edited since: update it. Changes asked for: the author edits; a reviewer may still decide.
	expect(lead('stale', author)).toEqual({ primary: ACTION.update, secondary: [] });
	expect(nextStep('changes_requested', false, author)).toEqual({ kind: 'changes-asked' });
	expect(nextStep('changes_requested', false, owner)).toEqual({ kind: 'decide' });
	expect(lead('published', owner)).toEqual({ primary: null, secondary: [] });
});

test('who a request waits for, and the task that asks them', () => {
	expect(names([])).toBe('');
	expect(names(['Sam'])).toBe('Sam');
	expect(names(['Sam', 'Alex'])).toBe('Sam and Alex');
	expect(names(['Sam', 'Alex', 'Kim'])).toBe('Sam, Alex and Kim');
	expect(reviewTaskTitle('Spring launch', '')).toBe('Review “Spring launch”');
	expect(reviewTaskTitle('Spring launch', '  check the dates  ')).toBe('Review “Spring launch”: check the dates');
	// The server keeps task titles to 200 characters: a long request title gives way to the note.
	const long = reviewTaskTitle('x'.repeat(200), 'the note stays whole');
	expect(long.length).toBeLessThanOrEqual(200);
	expect(long.endsWith('”: the note stays whole')).toBe(true);
});
