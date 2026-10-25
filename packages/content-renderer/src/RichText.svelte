<!--
  Renders a stored rich-text document. A thin wrapper around renderToHtml, so the Studio
  preview and the public site use one implementation (invariant R1). `{@html}` is safe here
  because renderToHtml escapes every piece of text and builds every tag itself.

  Without `fallback`, a document outside the format throws, as it should on the public site:
  validation should have stopped it (R3). The Studio previews drafts the server hasn't judged
  yet, so it passes a `fallback` that explains why a document can't be shown.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import type { RichTextValue } from '@colophon/editor/stored';
	import { renderToHtml, type RenderOptions, UnrenderableContent } from './render.ts';

	let {
		value,
		assetUrl,
		fallback
	}: { value: RichTextValue; assetUrl: RenderOptions['assetUrl']; fallback?: Snippet<[string]> } = $props();

	let result = $derived.by((): { html: string } | { problem: string } => {
		try {
			return { html: renderToHtml(value, { assetUrl }) };
		} catch (error) {
			if (fallback !== undefined && error instanceof UnrenderableContent) return { problem: error.message };
			throw error;
		}
	});
</script>

{#if 'html' in result}
	<div class="rich-text">{@html result.html}</div>
{:else if fallback}
	{@render fallback(result.problem)}
{/if}
