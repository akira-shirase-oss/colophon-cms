// Where an entry stands for readers, defined once (D-059's three states, said as two facts): an
// entry is live when readers get a version of it, and, separately, it has unpublished changes
// when its draft is not the version they get. "Live with changes" is live. The overview's tile,
// a collection's filters, the sidebar's counts and every badge read entries through this module,
// so no two screens can count them differently. Pure: no Svelte, no fetching.

import type { Summary } from './api.ts';

/** What one entry needs for its standing: its draft's revision and what readers get, if anything. */
export type Standing = Pick<Summary, 'revision' | 'published'>;

/** The two facts. */
export interface EntryFacts {
	/** Readers get a version of it. */
	live: boolean;
	/** Its draft differs from the version readers get (never true when it isn't live). */
	hasUnpublishedChanges: boolean;
}

export function facts(entry: Standing): EntryFacts {
	if (entry.published === null) return { live: false, hasUnpublishedChanges: false };
	return { live: true, hasUnpublishedChanges: entry.published.revision_id !== entry.revision.id };
}

/** The badge's one word: never published (or taken down), live as drafted, or live with changes. */
export type EntryState = 'draft' | 'live' | 'changed';

export function entryState(entry: Standing): EntryState {
	const { live, hasUnpublishedChanges } = facts(entry);
	if (!live) return 'draft';
	return hasUnpublishedChanges ? 'changed' : 'live';
}

/** A list's filters: live includes live with changes; drafts are what readers don't get. */
export type EntryFilter = 'all' | 'live' | 'changes' | 'draft';

export function shows(filter: EntryFilter, entry: Standing): boolean {
	const { live, hasUnpublishedChanges } = facts(entry);
	switch (filter) {
		case 'all':
			return true;
		case 'live':
			return live;
		case 'changes':
			return hasUnpublishedChanges;
		case 'draft':
			return !live;
	}
}

/** How many entries stand where. `live` counts those with changes too; `live + drafts === total`. */
export interface Tally {
	total: number;
	live: number;
	withChanges: number;
	drafts: number;
}

export function tally(entries: Iterable<Standing>): Tally {
	const counts: Tally = { total: 0, live: 0, withChanges: 0, drafts: 0 };
	for (const entry of entries) {
		const { live, hasUnpublishedChanges } = facts(entry);
		counts.total += 1;
		if (live) counts.live += 1;
		else counts.drafts += 1;
		if (hasUnpublishedChanges) counts.withChanges += 1;
	}
	return counts;
}

/** "3 entries · 1 live (1 with unpublished changes) · 2 drafts". `more`: the lists hold only their
 *  newest page, so every number is a floor ("100+ entries"). */
export function tallyText(counts: Tally, more = false): string {
	const n = (value: number) => `${value.toLocaleString()}${more ? '+' : ''}`;
	const changes = counts.withChanges > 0 ? ` (${n(counts.withChanges)} with unpublished changes)` : '';
	return [
		`${n(counts.total)} ${counts.total === 1 && !more ? 'entry' : 'entries'}`,
		`${n(counts.live)} live${changes}`,
		`${n(counts.drafts)} ${counts.drafts === 1 && !more ? 'draft' : 'drafts'}`
	].join(' · ');
}
