<!-- Where an entry stands for readers, as a word with a mark: never colour alone. -->
<script lang="ts">
	import { Badge } from '@colophon/ui/ui/badge';
	import type { EntryState } from '#lib/api.ts';

	let { state, generation = null }: { state: EntryState; generation?: number | null } = $props();

	const LOOK = {
		draft: { variant: 'draft', text: 'Draft', dot: 'bg-draft', title: 'Not published' },
		live: { variant: 'live', text: 'Live', dot: 'bg-live', title: 'Readers see this version' },
		changed: { variant: 'pending', text: 'Changed', dot: 'bg-pending', title: 'Edited since it went live' }
	} as const;
	const look = $derived(LOOK[state]);
</script>

<Badge variant={look.variant} data-testid="entry-state" data-state={state} title={generation === null ? look.title : `${look.title} (release ${generation})`}>
	<span class="size-1.5 rounded-full {look.dot}" aria-hidden="true"></span>{look.text}
</Badge>
