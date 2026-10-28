<!--
  The first step of adding a field (the content-type builder): what kind of value it holds, as a
  grid of cards in four groups (Basics, Links, Structure, Advanced), each with its icon, its name
  and one plain line. Every field type the schema has is here; what can't go in this place is
  shown, greyed, with the reason (a URL name only at the top and only once, structures at most
  three deep, a shared component only once one exists and fits).
-->
<script lang="ts">
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { type Component, type FieldType, MAX_DEPTH } from './api.ts';
	import { ABOUT, canNest, componentFits, NAMES, PICKER_GROUPS, STRUCTURES } from './fieldDefs.ts';
	import { FIELD_ICONS } from './fieldIcons.ts';

	let {
		open = $bindable(false),
		within = '',
		depth,
		hasSlug,
		components,
		insideComponent = false,
		onpick
	}: {
		open?: boolean;
		/** What the field goes in ("Hero"), or "" for the content type itself. */
		within?: string;
		depth: number;
		/** This place already has a URL name field. */
		hasSlug: boolean;
		/** Shared components (D-073); none inside a component. */
		components: Component[];
		/** These are a shared component's own fields, which can't hold another component. */
		insideComponent?: boolean;
		onpick: (kind: FieldType | 'component') => void;
	} = $props();

	const fitting = $derived(components.filter((shared) => componentFits(shared, depth)));

	/** Why a kind can't be added here, or "" when it can. */
	function why(kind: FieldType | 'component'): string {
		if (kind === 'slug' && depth > 0) return 'Only on the content type itself, not inside a group.';
		if (kind === 'slug' && hasSlug) return 'This content type already has one.';
		if (STRUCTURES.includes(kind as FieldType) && !canNest(depth)) return `Not this deep: groups nest at most ${MAX_DEPTH} deep.`;
		if (kind === 'component' && insideComponent) return 'A component can’t hold another component.';
		if (kind === 'component' && components.length === 0) return 'Make one first, under Components.';
		if (kind === 'component' && fitting.length === 0) return `Not this deep: groups nest at most ${MAX_DEPTH} deep.`;
		return '';
	}
	const label = (kind: FieldType | 'component') => (kind === 'component' ? 'Shared component' : NAMES[kind]);
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-2xl lg:max-w-3xl" data-testid="field-picker">
		<Dialog.Header>
			<Dialog.Title>{within === '' ? 'Add a field' : `Add a field to ${within}`}</Dialog.Title>
			<Dialog.Description>Choose what kind of value it holds. You name it next.</Dialog.Description>
		</Dialog.Header>
		<div class="grid gap-5">
			{#each PICKER_GROUPS as group (group.name)}
				<section aria-labelledby="picker-{group.name}">
					<h3 id="picker-{group.name}" class="eyebrow mb-2">{group.name}</h3>
					<ul class="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
						{#each group.kinds as kind (kind)}
							{@const Icon = FIELD_ICONS[kind]}
							{@const blocked = why(kind)}
							<li>
								<button
									type="button"
									class="group flex h-full w-full items-start gap-3 rounded-xl border border-border bg-card p-3 text-left shadow-hairline transition-colors outline-none hover:border-ring/60 hover:bg-accent focus-visible:border-ring focus-visible:ring-4 focus-visible:ring-ring/25 disabled:cursor-not-allowed disabled:bg-muted/40 disabled:shadow-none disabled:hover:border-border"
									disabled={blocked !== ''}
									onclick={() => onpick(kind)}
									data-testid="pick-{kind}"
								>
									<span
										class="flex size-9 shrink-0 items-center justify-center rounded-lg border border-border bg-background text-foreground group-disabled:text-muted-foreground"
										aria-hidden="true"><Icon class="size-4" /></span
									>
									<span class="grid min-w-0 gap-0.5">
										<span class="text-sm font-medium group-disabled:text-muted-foreground">{label(kind)}</span>
										<span class="text-xs text-muted-foreground">{blocked === '' ? ABOUT[kind] : blocked}</span>
									</span>
								</button>
							</li>
						{/each}
					</ul>
				</section>
			{/each}
			{#if !canNest(depth)}
				<p class="text-xs text-muted-foreground" data-testid="nesting-limit">
					Groups, lists and page sections aren't offered here: they nest at most {MAX_DEPTH} deep.
				</p>
			{/if}
		</div>
	</Dialog.Content>
</Dialog.Root>
