// Where an entry stands, defined once (lib/entryState.ts): live is "readers get a version", and
// unpublished changes are a separate fact, so an entry live with changes counts as live
// everywhere — the overview's tile, a list's filters and the sidebar's counts. The audit's F2:
// one article live and edited since read "0 live · 1 changed".

import { expect, test } from '@playwright/test';
import { entryState, facts, shows, tally, tallyText, type Standing } from '../src/lib/entryState.ts';

const revision = (id: string) => ({ id, base_revision_id: null, author_id: 'a', created_at: '2026-11-18T09:00:00Z', content_hash: 'sha256:x', title: id });

/** Never published (or taken down). */
const draft: Standing = { revision: revision('d1'), published: null };
/** Live as drafted. */
const live: Standing = { revision: revision('l1'), published: { revision_id: 'l1', generation: 1 } };
/** Live, edited since. */
const changed: Standing = { revision: revision('c2'), published: { revision_id: 'c1', generation: 1 } };

test('live means readers get a version; unpublished changes are a fact of their own', () => {
	expect(facts(draft)).toEqual({ live: false, hasUnpublishedChanges: false });
	expect(facts(live)).toEqual({ live: true, hasUnpublishedChanges: false });
	expect(facts(changed)).toEqual({ live: true, hasUnpublishedChanges: true });
	expect([draft, live, changed].map(entryState)).toEqual(['draft', 'live', 'changed']);
});

test('an entry live with changes counts as live, and the counts add up', () => {
	// F2's project: one article live and edited since, two drafts.
	const counts = tally([changed, draft, draft]);
	expect(counts).toEqual({ total: 3, live: 1, withChanges: 1, drafts: 2 });
	expect(tallyText(counts)).toBe('3 entries · 1 live (1 with unpublished changes) · 2 drafts');
	expect(tallyText(tally([live]))).toBe('1 entry · 1 live · 0 drafts');
	expect(tallyText(tally([]))).toBe('0 entries · 0 live · 0 drafts');
	// The lists hold their newest page: with more beyond it, every number is a floor.
	expect(tallyText({ total: 100, live: 40, withChanges: 3, drafts: 60 }, true)).toBe('100+ entries · 40+ live (3+ with unpublished changes) · 60+ drafts');
	for (const entries of [[], [draft], [live, changed], [changed, changed, draft, live]]) {
		const each = tally(entries);
		expect(each.live + each.drafts).toBe(each.total);
		expect(each.withChanges).toBeLessThanOrEqual(each.live);
	}
});

test('a list’s filters agree with the tally', () => {
	const entries = [draft, live, changed, changed];
	const counts = tally(entries);
	expect(entries.filter((entry) => shows('all', entry))).toHaveLength(counts.total);
	expect(entries.filter((entry) => shows('live', entry))).toEqual([live, changed, changed]);
	expect(entries.filter((entry) => shows('live', entry))).toHaveLength(counts.live);
	expect(entries.filter((entry) => shows('changes', entry))).toHaveLength(counts.withChanges);
	expect(entries.filter((entry) => shows('draft', entry))).toEqual([draft]);
	expect(entries.filter((entry) => shows('draft', entry))).toHaveLength(counts.drafts);
});
