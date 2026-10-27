<!--
  A field-by-field comparison: changed fields with their words struck out and added, unchanged
  fields folded away. Additions and removals carry their meaning in text for screen readers.
-->
<script lang="ts">
	import type { FieldChange } from './diff.ts';

	let { changes, before = 'Live', after = 'Pinned' }: { changes: FieldChange[]; before?: string; after?: string } = $props();

	const changed = $derived(changes.filter((change) => change.changed));
	const same = $derived(changes.length - changed.length);
</script>

<div class="space-y-4" data-testid="diff">
	{#each changed as change (change.field.id)}
		<div class="grid gap-1.5" data-testid="diff-field" data-field={change.field.id}>
			<p class="text-xs font-medium text-muted-foreground">{change.field.label}</p>
			<p class="text-sm leading-relaxed whitespace-pre-wrap">
				{#each change.pieces as piece, index (index)}
					{#if piece.kind === 'same'}<span>{piece.text}</span>{:else if piece.kind === 'removed'}<del
							class="rounded-sm bg-attention-soft px-0.5 text-attention-ink decoration-attention-ink/60"
							><span class="sr-only">removed: </span>{piece.text}</del
						>{:else}<ins class="rounded-sm bg-live-soft px-0.5 text-live-ink no-underline"><span class="sr-only">added: </span>{piece.text}</ins>{/if}
				{/each}
				{#if change.pieces.length === 0}<span class="text-muted-foreground italic">(empty)</span>{/if}
			</p>
		</div>
	{:else}
		<p class="text-sm text-muted-foreground">No differences between {before.toLowerCase()} and {after.toLowerCase()}.</p>
	{/each}
	{#if same > 0 && changed.length > 0}
		<p class="text-xs text-muted-foreground">{same} other field{same === 1 ? '' : 's'} unchanged.</p>
	{/if}
</div>
