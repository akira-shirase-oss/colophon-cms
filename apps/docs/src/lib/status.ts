// The website's three status words and what they promise (apps/site/src/lib/status.ts), so a page
// here and a feature there say the same thing in the same words. tests/look.spec.ts fails if the
// two drift.
export type StatusKind = 'done' | 'building' | 'planned';

export const STATUS_WORD: Record<StatusKind, string> = {
	done: 'Done',
	building: 'Building',
	planned: 'Planned'
};

export const STATUS_MEANING: Record<StatusKind, string> = {
	done: 'Merged into main, with its tests, testbed page and load scenario.',
	building: 'A step in progress on its own branch; not merged yet.',
	planned: 'In the roadmap, in the milestone shown; nothing built yet.'
};

/**
 * The same words on a milestone, which is never on a branch: a milestone is under way while some
 * of its steps are merged and its proof gate isn't met.
 */
export const MILESTONE_MEANING: Record<StatusKind, string> = {
	done: 'Its proof gate passed, with stored evidence.',
	building: 'Under way: some of its steps are merged; its proof gate isn’t met yet.',
	planned: 'In the roadmap; nothing in it is built yet.'
};
