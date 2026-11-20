// The Activity log names an undo of a takedown for the release it undid (D-184), never as a
// rollback, and survives a detail it doesn't expect.

import { expect, test } from '@playwright/test';
import type { AuditEvent } from '../src/lib/api.ts';
import { KINDS, type Names, sentence } from '../src/lib/activity.ts';

const names: Names = { person: () => undefined, role: (key) => key, invitation: () => undefined, types: [] };
const event = (detail: Record<string, unknown>): AuditEvent => ({
	id: 'e1',
	at: '2026-11-18T14:00:00Z',
	actor: { id: 'p1', name: 'Dev Owner', kind: 'person' },
	action: 'release.undone',
	subject: '4',
	detail
});

test('an undo is said as an undo of its release, with what it put back and where', () => {
	expect(sentence(event({ generation: 5, undo_of: 4, entries: 1, title: 'Unpublish: Tide tables' }), names)).toBe(
		'Dev Owner undid Release 4 (“Unpublish: Tide tables”), putting back what it took down as Release 5.'
	);
	expect(sentence(event({}), names)).toBe('Dev Owner undid Release 4, putting back what it took down.');
	expect(sentence(event({ generation: 'five', title: '' }), names)).toBe('Dev Owner undid Release 4, putting back what it took down.');
	expect(sentence(event({ generation: 5 }), names)).not.toMatch(/rollback|roll back/i);
	expect(KINDS.some((kind) => kind.action === 'release.undone')).toBe(true);
});
