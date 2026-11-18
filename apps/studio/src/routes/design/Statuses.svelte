<!-- Every state the one StatusBadge knows (lib/status.ts), grouped by what it describes. -->
<script lang="ts">
	import StatusBadge from '#lib/shell/StatusBadge.svelte';
	import type { Status } from '#lib/status.ts';
	import Specimen from './Specimen.svelte';

	const GROUPS: { label: string; states: { state: Status; text?: string }[] }[] = [
		{ label: 'An entry, for readers', states: [{ state: 'live' }, { state: 'changed' }, { state: 'draft' }] },
		{ label: 'On its way', states: [{ state: 'scheduled' }, { state: 'review' }] },
		{ label: 'What a release does to an entry', states: [{ state: 'new' }, { state: 'updated' }, { state: 'taken_down' }] },
		{
			label: "A release's label",
			states: [
				{ state: 'release_published' },
				{ state: 'release_pending' },
				{ state: 'release_verified', text: 'Verified at 2 locations' },
				{ state: 'release_attention' }
			]
		},
		{
			label: 'A publish request',
			states: [
				{ state: 'request_open' },
				{ state: 'request_review' },
				{ state: 'request_changes' },
				{ state: 'request_approved' },
				{ state: 'request_stale' },
				{ state: 'request_published' }
			]
		},
		{ label: 'A schedule', states: [{ state: 'published' }, { state: 'published_late' }, { state: 'not_published' }, { state: 'cancelled' }] }
	];
</script>

<div class="grid gap-10 md:grid-cols-2">
	{#each GROUPS as group (group.label)}
		<Specimen label={group.label}>
			<div class="flex flex-wrap items-center gap-2">
				{#each group.states as { state, text } (state)}
					<StatusBadge {state} {text} testid="status-{state}" />
				{/each}
			</div>
		</Specimen>
	{/each}
</div>
