// Undo, rollback or neither (lib/releaseOrigin.ts). An undo of release N carries
// `rollback_generation = N - 1` too (D-184), so the label must come from `undo_of` first.

import { expect, test } from '@playwright/test';
import { originRelease, originText, releaseOrigin } from '../src/lib/releaseOrigin.ts';

test('an undo reads as an undo, never as a rollback to the release before', () => {
	const undo = releaseOrigin({ undo_of: 5, rollback_generation: 4 });
	expect(undo).toEqual({ kind: 'undo', of: 5 });
	expect(originText(undo)).toBe('Undo of Release 5');
	expect(originRelease(undo)).toBe(5);
});

test('a rollback reads as a rollback', () => {
	const rollback = releaseOrigin({ undo_of: null, rollback_generation: 2 });
	expect(originText(rollback)).toBe('Rollback to Release 2');
	expect(originRelease(rollback)).toBe(2);
	// A server without `undo_of` yet (the field is optional until D-184 lands).
	expect(originText(releaseOrigin({ rollback_generation: 2 }))).toBe('Rollback to Release 2');
});

test('an ordinary request or release has no origin', () => {
	expect(releaseOrigin({ rollback_generation: null })).toBeNull();
	expect(releaseOrigin({})).toBeNull();
	expect(originText(null)).toBeNull();
	expect(originRelease(null)).toBeNull();
});
