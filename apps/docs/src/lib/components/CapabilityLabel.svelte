<!--
  A page's or a part's status (module 16, DS1), drawn exactly like the website's StatusBadge: Done,
  Building or Planned, with the step, branch or milestone, as a pill with a word and a dot, never
  colour alone. Overview and design-document pages say what they are instead. The title says what
  the label promises.
-->
<script lang="ts">
	import { Badge } from '@colophon/ui/ui/badge';
	import type { Capability } from '#lib/nav.ts';
	import { MILESTONE_MEANING, STATUS_MEANING, STATUS_WORD } from '#lib/status.ts';

	let { capability, size = 'default' }: { capability: Capability; size?: 'default' | 'small' } = $props();

	/** The roadmap's milestones: P0 to P6 and the lettered ones (docs/ROADMAP.md). */
	const MILESTONE = /^(P[0-6]|[RMEDSV])$/;

	const look = $derived.by(() => {
		switch (capability.kind) {
			case 'done':
				return { variant: 'live', dot: 'bg-live', word: STATUS_WORD.done, at: capability.at ?? '', title: STATUS_MEANING.done } as const;
			case 'building':
				// A milestone under way (P3, R, E) is never on a branch; a step or branch is.
				return {
					variant: 'pending',
					dot: 'bg-pending breathe',
					word: STATUS_WORD.building,
					at: capability.at,
					title: MILESTONE.test(capability.at) ? MILESTONE_MEANING.building : STATUS_MEANING.building
				} as const;
			case 'planned':
				return { variant: 'draft', dot: 'bg-draft', word: STATUS_WORD.planned, at: capability.at, title: STATUS_MEANING.planned } as const;
			case 'overview':
				return { variant: 'outline', dot: 'bg-muted-foreground', word: 'Overview', at: '', title: 'Introduces other pages; each part says where it stands' } as const;
			case 'design':
				return { variant: 'brand', dot: 'bg-brand', word: 'Design document', at: '', title: 'A plan from docs/: it mixes what is built with what is planned' } as const;
		}
	});
</script>

<Badge
	variant={look.variant}
	title={look.title}
	data-capability={capability.kind}
	data-status={capability.kind}
	class={size === 'small' ? 'px-2 py-0 text-[0.6875rem]' : ''}
>
	<span class="size-1.5 rounded-full {look.dot}" aria-hidden="true"></span>
	{look.word}{#if look.at}<span class="font-normal"> · {look.at}</span>{/if}
</Badge>
