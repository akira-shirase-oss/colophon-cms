<!--
  Where something stands, as a word with an icon on its tone's soft ground: never colour alone
  (D-182). The one badge for an entry, a publish request, a release and a schedule: `state` names
  a state in lib/status.ts, which gives its tone, icon and word. An entry's badge is
  `data-testid="entry-state"` unless another id is given; `data-state` always carries the state.
-->
<script lang="ts">
	import type { HTMLAttributes } from 'svelte/elements';
	import { cn } from '@colophon/ui/utils';
	import { STATUS, type Status, TONE_CLASS } from '#lib/status.ts';

	let {
		state,
		text,
		title,
		generation = null,
		href,
		testid,
		class: className,
		...rest
	}: {
		state: Status;
		/** Words that say more than the state's own, such as "Verified at 2 locations". */
		text?: string;
		title?: string;
		/** For an entry: the release readers have it from, named in the title. */
		generation?: number | null;
		href?: string;
		testid?: string;
		class?: string;
	} & Omit<HTMLAttributes<HTMLElement>, 'title'> = $props();

	const ENTRY_STATES = new Set<Status>(['live', 'changed', 'draft']);
	const look = $derived(STATUS[state]);
	const tone = $derived(TONE_CLASS[look.tone]);
	const said = $derived(title ?? (generation === null ? look.title : `${look.title} (release ${generation})`));
</script>

<svelte:element
	this={href ? 'a' : 'span'}
	{href}
	data-slot="status-badge"
	data-tone={look.tone}
	data-testid={testid ?? (ENTRY_STATES.has(state) ? 'entry-state' : undefined)}
	data-state={state}
	title={said}
	class={cn(
		'inline-flex h-6 w-fit shrink-0 items-center gap-1.5 rounded-full px-2.5 text-xs font-medium whitespace-nowrap text-foreground ring-1 ring-inset',
		href ? 'transition-colors hover:underline focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none' : '',
		tone.badge,
		className
	)}
	{...rest}
>
	<look.icon class="size-3.5 shrink-0 {tone.icon}" aria-hidden="true" stroke-width={2.25} />{text ?? look.text}
</svelte:element>
