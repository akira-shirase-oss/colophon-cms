<!-- A publish request's state (D-044) as its StatusBadge; `data-state` keeps the API's name. -->
<script lang="ts">
	import type { ChangesetState } from '#lib/api.ts';
	import { requestStatus } from '#lib/status.ts';
	import StatusBadge from './StatusBadge.svelte';

	let { state, testid }: { state: ChangesetState; testid?: string } = $props();
</script>

<StatusBadge state={requestStatus(state)} {testid} data-state={state} />
