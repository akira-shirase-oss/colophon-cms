<!--
  A status inside a Markdown page, for a part that differs from the page's own label:
  `<Status done="2.1" />`, `<Status building="3.6" />`, `<Status building="e/schedules" />` or
  `<Status planned="R.3" />`.
-->
<script lang="ts">
	import type { Capability } from '#lib/nav.ts';
	import CapabilityLabel from './CapabilityLabel.svelte';

	let { done, building, planned }: { done?: string | boolean; building?: string; planned?: string } = $props();

	const capability: Capability = $derived(
		planned !== undefined
			? { kind: 'planned', at: planned }
			: building !== undefined
				? { kind: 'building', at: building }
				: typeof done === 'string' && done !== ''
					? { kind: 'done', at: done }
					: { kind: 'done' }
	);
</script>

<span class="ml-1.5 inline-flex align-middle"><CapabilityLabel {capability} size="small" /></span>
