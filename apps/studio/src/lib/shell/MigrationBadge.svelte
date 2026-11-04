<!-- Where a migration plan stands, as a word with a mark: never colour alone (D-100). -->
<script lang="ts">
	import { Badge } from '@colophon/ui/ui/badge';
	import type { MigrationState } from '#lib/api.ts';

	let { state }: { state: MigrationState } = $props();

	const LOOK = {
		proposed: { variant: 'pending', text: 'Proposed', title: 'Waiting to be approved' },
		approved: { variant: 'live', text: 'Approved', title: 'Ready to run' },
		running: { variant: 'pending', text: 'Running', title: 'A worker is applying it' },
		done: { variant: 'outline', text: 'Done', title: 'Every entry is handled' },
		blocked: { variant: 'attention', text: 'Blocked', title: 'It stopped partway: check its items' },
		superseded: { variant: 'draft', text: 'Superseded', title: 'A newer plan or change replaced it' },
		cancelled: { variant: 'draft', text: 'Cancelled', title: 'Called off before it ran' }
	} as const;
	const look = $derived(LOOK[state]);
</script>

<Badge variant={look.variant} data-testid="migration-state" data-state={state} title={look.title}>{look.text}</Badge>
