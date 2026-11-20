// Where a publish request or a release came from, in words: an Undo of one release (D-184), a
// rollback to an earlier one (D-097), or neither. An undo also carries `rollback_generation`
// (the release before the one it undoes), so `undo_of` is read first; reading only
// `rollback_generation` would call "Undo of Release 5" a "Rollback to Release 4".

import { releaseName } from './words.ts';

export type Origin = { kind: 'undo'; of: number } | { kind: 'rollback'; to: number } | null;

export function releaseOrigin(from: { undo_of?: number | null; rollback_generation?: number | null }): Origin {
	if (typeof from.undo_of === 'number') return { kind: 'undo', of: from.undo_of };
	if (typeof from.rollback_generation === 'number') return { kind: 'rollback', to: from.rollback_generation };
	return null;
}

/** "Undo of Release 5", "Rollback to Release 2", or `null` for an ordinary request. */
export function originText(origin: Origin): string | null {
	if (origin === null) return null;
	return origin.kind === 'undo' ? `Undo of ${releaseName(origin.of)}` : `Rollback to ${releaseName(origin.to)}`;
}

/** The release a link from the origin should open: the one undone, or the one restored. */
export function originRelease(origin: Origin): number | null {
	if (origin === null) return null;
	return origin.kind === 'undo' ? origin.of : origin.to;
}
