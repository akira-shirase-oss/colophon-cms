<!-- A release's one label (D-051) as a pill; "Verified" always says how many locations looked. -->
<script lang="ts">
	import BadgeCheckIcon from '@lucide/svelte/icons/badge-check';
	import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { Badge } from '@colophon/ui/ui/badge';
	import type { Release } from '#lib/api.ts';
	import { labelText } from '#lib/receipts.ts';

	let { release, href, testid = 'label' }: { release: Pick<Release, 'label' | 'locations'>; href?: string; testid?: string } = $props();

	const LOOK = {
		published: { variant: 'draft', icon: CircleDashedIcon },
		pending: { variant: 'pending', icon: ClockIcon },
		verified: { variant: 'live', icon: BadgeCheckIcon },
		needs_attention: { variant: 'attention', icon: TriangleAlertIcon }
	} as const;
	const look = $derived(LOOK[release.label]);
</script>

<Badge variant={look.variant} {href} data-testid={testid} data-label={release.label}>
	<look.icon aria-hidden="true" />{labelText(release)}
</Badge>
