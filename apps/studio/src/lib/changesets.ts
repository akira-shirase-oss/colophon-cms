// Words for a changeset's states (D-044), shared by the list and the changeset page.

import type { ChangesetState } from './api.ts';

export const STATE_LABEL: Record<ChangesetState, string> = {
	open: 'Not submitted',
	stale: 'Edited since pinned: refresh',
	awaiting_review: 'Awaiting review',
	changes_requested: 'Changes requested',
	approved: 'Approved',
	approval_expired: 'Approval expired'
};
