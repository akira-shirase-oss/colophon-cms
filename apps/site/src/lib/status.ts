// Every feature on the site carries one of three words (the project's honesty rule): done (merged
// into main, with its proof), building (a step in progress) or planned (the milestone that will
// build it). When in doubt, a feature is planned or left out.
export type StatusKind = 'done' | 'building' | 'planned';

export interface Status {
	kind: StatusKind;
	/** Where: the step or milestone, e.g. "P2", "3.6", "R". */
	at: string;
}

export const done = (at: string): Status => ({ kind: 'done', at });
export const building = (at: string): Status => ({ kind: 'building', at });
export const planned = (at: string): Status => ({ kind: 'planned', at });

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
