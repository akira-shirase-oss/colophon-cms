<!--
  Rich text inside a structure (D-066): the same editor and toolbar as an entry's own rich text,
  holding its value in the stored format. A document the editor can't give back exactly isn't
  opened (D-022): it says so and leaves the value as it is. The parent re-creates it when the whole
  entry is replaced (a restore, their version), so it never fights an outside change.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import type { Editor } from '@tiptap/core';
	import { openEditor, type RichTextValue, toStored } from '@colophon/editor';
	import { EMPTY_DOCUMENT, type FormValue } from './content.ts';
	import RichTextToolbar from './RichTextToolbar.svelte';

	let {
		value = $bindable(),
		label,
		labelledBy,
		disabled = false,
		onchange = () => {}
	}: { value: FormValue; label: string; labelledBy?: string; disabled?: boolean; onchange?: () => void } = $props();

	let host = $state<HTMLDivElement | null>(null);
	let editor = $state.raw<Editor | null>(null);
	let unopenable = $state('');

	onMount(() => {
		try {
			const opened = openEditor(((value as unknown as RichTextValue | null) ?? EMPTY_DOCUMENT) as RichTextValue, {
				editorProps: {
					attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': label, class: 'prose-colophon min-h-32' }
				},
				editable: !disabled,
				onUpdate: () => {
					value = toStored(opened.getJSON()) as unknown as FormValue;
					onchange();
				}
			});
			if (host) opened.mount(host);
			editor = opened;
		} catch (error) {
			unopenable = (error as Error).message;
		}
		return () => editor?.destroy();
	});
</script>

{#if unopenable}
	<p role="alert" class="rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2 text-sm text-destructive">
		This can't be opened in the editor without changing it: {unopenable}
	</p>
{/if}
<div class="overflow-hidden rounded-xl border border-border bg-card shadow-xs focus-within:border-ring/60" role="group" aria-labelledby={labelledBy}>
	<RichTextToolbar {editor} label="Formatting: {label}" />
	<div class="editor px-4 py-3" bind:this={host}></div>
</div>
