<!--
  One version of an entry as a reader reads it, read-only and unmarked: each field that has a
  value under its label, rich text rendered by the content renderer the public site uses. For a
  new entry in a publish request or a receipt, where a comparison would mark every word as added.
-->
<script lang="ts">
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import type { RichTextValue } from '@colophon/editor';
	import { assetUrl as mediaUrl, type Content, type Definition } from './api.ts';
	import { readable } from './diff.ts';

	let { project, definition, content }: { project: string; definition: Definition; content: Content } = $props();

	const assetUrl = (asset: string) => mediaUrl(project, asset);
	const filled = $derived(definition.fields.filter((field) => content[field.id] !== undefined && content[field.id] !== null && content[field.id] !== ''));
</script>

<div class="grid gap-3" data-testid="version-preview">
	{#each filled as field (field.id)}
		<div class="grid gap-1">
			<p class="text-xs font-medium text-muted-foreground">{field.label}</p>
			{#if field.type === 'rich_text'}
				<div class="prose-colophon text-sm">
					<RichText value={content[field.id] as RichTextValue} {assetUrl}>
						{#snippet fallback(problem)}<p>This can't be shown: {problem}</p>{/snippet}
					</RichText>
				</div>
			{:else}
				<p class="text-sm leading-relaxed whitespace-pre-wrap" data-user-content>{readable(field, content[field.id])}</p>
			{/if}
		</div>
	{:else}
		<p class="text-sm text-muted-foreground">Every field is empty.</p>
	{/each}
</div>
