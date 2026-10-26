<!--
  The editor playground: every editing feature on the shared rich-text fixtures, with the
  canonical JSON Colophon would store and a preview from the renderer the public site uses.
  Nothing is saved.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { asset } from '$app/paths';
	import type { Editor } from '@tiptap/core';
	import { LIMITS, measure, openEditor, type RichTextValue, serialize, toStored } from '@colophon/editor';
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import { Button } from '@colophon/ui/ui/button';
	import * as Tabs from '@colophon/ui/ui/tabs';
	import { fixtures } from '#lib/fixtures.ts';
	import RichTextToolbar from '#lib/RichTextToolbar.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';

	// Until media uploads arrive (P4), every image previews as the same placeholder.
	const assetUrl = (assetId: string) => `${asset('placeholder.svg')}#${assetId}`;

	const EMPTY: RichTextValue = {
		schema: 'colophon/rich-text',
		version: 1,
		doc: { type: 'doc', content: [{ type: 'paragraph' }] }
	};

	let host: HTMLDivElement;
	let editor = $state.raw<Editor | null>(null);
	let value = $state.raw<RichTextValue>(EMPTY);
	let selectedFixture = $state('');
	let status = $state<{ kind: 'opened' | 'refused'; message: string } | null>(null);
	let tab = $state('json');

	let canonical = $derived(serialize(value));
	let size = $derived(measure(value));
	let pretty = $derived(JSON.stringify(value, null, 2));

	/** Opens `next` (refusing anything the editor can't reproduce exactly) and swaps it in. */
	function load(next: RichTextValue, label: string) {
		let created: Editor;
		try {
			created = openEditor(next, {
				// The editing area is a multi-line text box; give it a name screen readers announce.
				editorProps: {
					attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': 'Article body', class: 'prose-colophon min-h-[22rem]' }
				},
				onTransaction: () => {
					value = toStored(created.getJSON());
				}
			});
		} catch (error) {
			status = { kind: 'refused', message: `${label}: ${(error as Error).message}` };
			return;
		}
		editor?.destroy();
		created.mount(host);
		editor = created;
		value = toStored(created.getJSON());
		status = { kind: 'opened', message: `${label}: opened` };
	}

	onMount(() => {
		load(EMPTY, 'new document');
		return () => editor?.destroy();
	});

	function loadFixture() {
		const fixture = fixtures.find((candidate) => candidate.id === selectedFixture);
		if (fixture) load(fixture.value, fixture.id);
	}

	const select =
		'h-9 min-w-0 flex-1 rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40';
</script>

<svelte:head><title>Editor playground · Colophon Studio</title></svelte:head>

<PageHeader title="Editor playground">
	{#snippet description()}Every editing feature, on the shared fixtures. What you see on the right is exactly what Colophon would store and serve. Nothing is saved.{/snippet}
</PageHeader>

<section class="grid items-start gap-6 lg:grid-cols-[minmax(0,3fr)_minmax(0,2fr)]">
	<div class="overflow-hidden rounded-xl border border-border bg-card shadow-xs">
		<RichTextToolbar {editor} />
		<div class="editor px-6 py-5 sm:px-8" bind:this={host} data-testid="editor"></div>
		<p class="border-t border-dotted border-border px-4 py-2 font-mono text-xs text-muted-foreground tabular" data-testid="limits">
			Depth {size.depth}/{LIMITS.maxDepth} · {size.nodes} nodes/{LIMITS.maxNodes} · {size.bytes.toLocaleString()} bytes/{LIMITS.maxBytes.toLocaleString()}
		</p>
	</div>

	<aside class="space-y-4">
		<div>
			<h2 class="mb-2 text-sm font-semibold">Load a shared fixture</h2>
			<div class="flex items-center gap-2">
				<select class={select} bind:value={selectedFixture} aria-label="Fixture" data-testid="fixture-select">
					<option value="">Choose…</option>
					{#each ['valid', 'invalid'] as kind (kind)}
						<optgroup label={kind}>
							{#each fixtures.filter((fixture) => fixture.kind === kind) as fixture (fixture.id)}
								<option value={fixture.id}>{fixture.name}</option>
							{/each}
						</optgroup>
					{/each}
				</select>
				<Button variant="outline" onclick={loadFixture} disabled={selectedFixture === ''}>Open</Button>
			</div>
			{#if status}
				<p
					class="mt-2 rounded-lg px-3 py-2 text-sm break-words {status.kind === 'opened' ? 'bg-live-soft text-live-ink' : 'bg-attention-soft text-attention-ink'}"
					data-testid="open-status"
					data-kind={status.kind}
				>
					{status.message}
				</p>
			{/if}
		</div>

		<Tabs.Root bind:value={tab}>
			<Tabs.List aria-label="Output">
				<Tabs.Trigger value="json">Stored JSON</Tabs.Trigger>
				<Tabs.Trigger value="preview">Preview</Tabs.Trigger>
			</Tabs.List>
			<Tabs.Content value="json" class="mt-3">
				<p class="mb-2 text-xs text-muted-foreground">What Colophon would save: canonical colophon/rich-text v1.</p>
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<!-- A scrollable region must be reachable by keyboard (WCAG 2.1.1; axe scrollable-region-focusable). -->
				<pre
					class="max-h-[34rem] overflow-auto rounded-lg border border-border bg-muted/50 p-3 font-mono text-xs"
					data-testid="stored"
					data-json={canonical}
					tabindex="0"
					aria-label="Stored JSON">{pretty}</pre>
			</Tabs.Content>
			<Tabs.Content value="preview" class="mt-3">
				<div class="rounded-lg border border-border bg-card px-5 py-4" data-testid="preview">
					<p class="mb-3 font-sans text-xs text-muted-foreground">Rendered by the same renderer the public site uses.</p>
					<div class="prose-colophon">
						<RichText {value} {assetUrl}>
							{#snippet fallback(problem)}
								<p class="rounded-lg bg-attention-soft px-3 py-2 font-sans text-sm text-attention-ink" data-testid="preview-refused">
									This document can't be previewed: {problem}. The server would refuse it too.
								</p>
							{/snippet}
						</RichText>
					</div>
				</div>
			</Tabs.Content>
		</Tabs.Root>
	</aside>
</section>
