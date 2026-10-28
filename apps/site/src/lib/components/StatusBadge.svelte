<!-- Done, Building or Planned, with where: always a word, never colour alone. `of="milestone"`
     gives a milestone's meaning as the title: under way, not on a branch. -->
<script lang="ts">
	import { Badge } from '@colophon/ui/ui/badge';
	import { MILESTONE_MEANING, STATUS_MEANING, STATUS_WORD, type StatusKind } from '#lib/status.ts';

	let { kind, at = '', note = '', of = 'step' }: { kind: StatusKind; at?: string; note?: string; of?: 'step' | 'milestone' } = $props();

	const VARIANT = { done: 'live', building: 'pending', planned: 'draft' } as const;
	const DOT = { done: 'bg-live', building: 'bg-pending breathe', planned: 'bg-draft' } as const;
</script>

<Badge variant={VARIANT[kind]} title={(of === 'milestone' ? MILESTONE_MEANING : STATUS_MEANING)[kind]} data-status={kind}>
	<span class="size-1.5 rounded-full {DOT[kind]}" aria-hidden="true"></span>
	{STATUS_WORD[kind]}{#if at}<span class="font-normal opacity-100"> · {at}</span>{/if}{#if note}<span class="font-normal"> · {note}</span>{/if}
</Badge>
