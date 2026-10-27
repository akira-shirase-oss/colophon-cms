<!--
  A capability label inside a Markdown page, for a part that differs from the page's own label:
  `<Status planned="P4" />`, `<Status building="step 3.6" />` or `<Status implemented />`.
-->
<script lang="ts">
	import type { Capability } from '#lib/nav.ts';
	import CapabilityLabel from './CapabilityLabel.svelte';

	let { implemented = false, building, planned }: { implemented?: boolean; building?: string; planned?: string } = $props();

	const capability: Capability = $derived(
		planned !== undefined
			? { kind: 'planned', milestone: planned }
			: building !== undefined
				? { kind: 'building', step: building }
				: { kind: implemented ? 'implemented' : 'overview' }
	);
</script>

<span class="ml-1.5 inline-flex align-middle"><CapabilityLabel {capability} size="small" /></span>
