<!--
  A capability label (module 16, DS1): Implemented, Building or Planned with its milestone, as a
  pill with a word and a mark, never colour alone. Overview and design-document pages say what
  they are instead. The title says what the label promises.
-->
<script lang="ts">
	import { Badge } from '@colophon/ui/ui/badge';
	import type { Capability } from '#lib/nav.ts';

	let { capability, size = 'default' }: { capability: Capability; size?: 'default' | 'small' } = $props();

	const look = $derived.by(() => {
		switch (capability.kind) {
			case 'implemented':
				return { variant: 'live', dot: 'bg-live', text: 'Implemented', title: 'Built, with tests, in the code this site was built from' } as const;
			case 'building':
				return { variant: 'pending', dot: 'bg-pending', text: `Building · ${capability.step}`, title: 'In progress on its own branch; not in this code yet' } as const;
			case 'planned':
				return { variant: 'draft', dot: 'bg-draft', text: `Planned · ${capability.milestone}`, title: 'Designed, not built yet' } as const;
			case 'overview':
				return { variant: 'outline', dot: 'bg-muted-foreground', text: 'Overview', title: 'Introduces other pages; each part says what is built' } as const;
			case 'design':
				return { variant: 'brand', dot: 'bg-brand', text: 'Design document', title: 'A plan from docs/: it mixes what is built with what is planned' } as const;
		}
	});
</script>

<Badge
	variant={look.variant}
	title={look.title}
	data-capability={capability.kind}
	class={size === 'small' ? 'px-2 py-0 text-[0.6875rem]' : ''}
>
	<span class="size-1.5 rounded-full {look.dot}" aria-hidden="true"></span>{look.text}
</Badge>
