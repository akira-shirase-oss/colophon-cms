<!-- The four labels a release can carry (D-051), in the Studio's words. -->
<script lang="ts">
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { Badge } from '@colophon/ui/ui/badge';

	let { class: className = '' }: { class?: string } = $props();
</script>

<ul class="flex flex-wrap gap-2 {className}" aria-label="The four labels a release can carry">
	<li><Badge variant="draft"><CircleDashedIcon aria-hidden="true" />Published in CMS</Badge></li>
	<li><Badge variant="pending"><ClockIcon aria-hidden="true" class="breathe" />Website update pending</Badge></li>
	<li><Badge variant="live"><BadgeCheckIcon aria-hidden="true" />Verified at 1 location</Badge></li>
	<li><Badge variant="attention"><TriangleAlertIcon aria-hidden="true" />Needs attention</Badge></li>
</ul>
