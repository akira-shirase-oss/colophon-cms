<!-- A release's one label (D-051) as its StatusBadge; "Verified" always says how many locations looked. -->
<script lang="ts">
	import type { Release } from '#lib/api.ts';
	import { labelText } from '#lib/receipts.ts';
	import { releaseStatus } from '#lib/status.ts';
	import StatusBadge from './StatusBadge.svelte';

	let { release, href, testid = 'label' }: { release: Pick<Release, 'label' | 'locations'>; href?: string; testid?: string } = $props();
</script>

<StatusBadge state={releaseStatus(release.label)} text={labelText(release)} {href} {testid} data-label={release.label} />
