<!--
  A field-by-field comparison: changed fields with their words struck out and added, unchanged
  fields folded away. Rich text is compared block by block and drawn in its shape — a heading as a
  heading, a list item with its bullet, a quote set in — so the structure survives the comparison
  (the audit's P2-13). Additions and removals carry their meaning in text for screen readers.
-->
<script lang="ts">
	import type { FieldChange, Piece } from './diff.ts';

	let { changes, before = 'Live', after = 'This request' }: { changes: FieldChange[]; before?: string; after?: string } = $props();

	const changed = $derived(changes.filter((change) => change.changed));
	const same = $derived(changes.length - changed.length);
	const SHAPE: Record<string, string> = {
		paragraph: '',
		heading: 'font-display text-base font-medium',
		item: 'relative pl-4 before:absolute before:left-1 before:content-["•"] before:text-muted-foreground',
		quote: 'border-l-2 border-border pl-3',
		callout: 'rounded-md bg-muted px-3 py-2',
		code: 'font-mono text-xs whitespace-pre-wrap',
		rule: 'text-muted-foreground',
		image: 'text-muted-foreground italic'
	};
</script>

{#snippet words(pieces: Piece[])}
	{#each pieces as piece, index (index)}
		{#if piece.kind === 'same'}<span>{piece.text}</span>{:else if piece.kind === 'removed'}<del
				class="rounded-sm bg-attention-soft px-0.5 text-attention-ink decoration-attention-ink/60"
				><span class="sr-only">removed: </span>{piece.text}</del
			>{:else}<ins class="rounded-sm bg-live-soft px-0.5 text-live-ink no-underline"><span class="sr-only">added: </span>{piece.text}</ins>{/if}
	{/each}
{/snippet}

<div class="space-y-4" data-testid="diff">
	{#each changed as change (change.field.id)}
		<div class="grid gap-1.5" data-testid="diff-field" data-field={change.field.id}>
			<p class="text-xs font-medium text-muted-foreground">{change.field.label}</p>
			{#if change.blocks !== undefined && change.blocks.length > 0}
				<div class="grid gap-2 text-sm leading-relaxed" data-testid="diff-blocks">
					{#each change.blocks as block, index (index)}
						{#if block.status !== 'same' || block.pieces.length > 0}
							<p class="{SHAPE[block.kind] ?? ''} {block.status === 'same' ? 'text-muted-foreground' : ''}" data-block={block.kind} data-status={block.status}>
								{@render words(block.pieces)}
							</p>
						{/if}
					{/each}
				</div>
			{:else}
				<p class="text-sm leading-relaxed whitespace-pre-wrap">
					{@render words(change.pieces)}
					{#if change.pieces.length === 0}<span class="text-muted-foreground">(empty)</span>{/if}
				</p>
			{/if}
		</div>
	{:else}
		<p class="text-sm text-muted-foreground">No differences between {before.toLowerCase()} and {after.toLowerCase()}.</p>
	{/each}
	{#if same > 0 && changed.length > 0}
		<p class="text-xs text-muted-foreground">{same} other field{same === 1 ? '' : 's'} unchanged.</p>
	{/if}
</div>
