<script lang="ts">
	import { Badge } from '@colophon/ui/ui/badge';
	import type { ChangesetState } from '#lib/api.ts';
	import { STATE_LABEL } from '#lib/changesets.ts';

	let { state }: { state: ChangesetState } = $props();

	const VARIANT = {
		open: 'draft',
		stale: 'attention',
		awaiting_review: 'pending',
		changes_requested: 'attention',
		approved: 'live',
		approval_expired: 'attention',
		published: 'outline'
	} as const;
</script>

<Badge variant={VARIANT[state]} data-state={state}>{STATE_LABEL[state]}</Badge>
