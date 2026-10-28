<!--
  What readers get for one entry (gap audit, 2026-10-28): the public read of it, made the way a
  website makes it (no sign-in), at the latest release. It says in words which release that is and
  whether newer changes are left out, then shows the answer exactly as a website receives it, to
  copy for whoever builds the site. Taken down, not out yet, or no answer: each said plainly.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, readDelivered } from './api.ts';
	import Problem from './shell/Problem.svelte';
	import { capital, releaseName, URL_NAME } from './words.ts';

	let {
		open = $bindable(false),
		project,
		collection,
		entry,
		changed,
		titleLabel = 'Title'
	}: {
		open?: boolean;
		project: string;
		collection: string;
		entry: string;
		/** The draft has changes since the published version. */
		changed: boolean;
		/** What the type calls each entry (D-082): "Title", "Name". */
		titleLabel?: string;
	} = $props();

	// Asked each time it opens: a release may have gone out since.
	const read = createQuery(() => ({
		queryKey: ['delivered', project, collection, entry],
		queryFn: () => readDelivered(project, collection, entry),
		enabled: open,
		staleTime: 0
	}));
	const pretty = $derived(read.data ? JSON.stringify(JSON.parse(read.data.raw), null, 2) : '');
	const gone = $derived(read.error instanceof ApiError && read.error.status === 410);
	const notYet = $derived(read.error instanceof ApiError && read.error.status === 404);

	async function copy() {
		try {
			await navigator.clipboard.writeText(pretty);
			toast.success('Copied');
		} catch {
			toast.error("Couldn't copy; select it and copy it yourself.");
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-2xl" data-testid="reader-view">
		<Dialog.Header>
			<Dialog.Title>What readers get</Dialog.Title>
			<Dialog.Description>
				What your website receives for this entry right now, read the way it reads it, without signing in.
			</Dialog.Description>
		</Dialog.Header>
		{#if read.isPending}
			<Skeleton class="h-40 w-full rounded-xl" />
		{:else if gone}
			<p class="rounded-lg bg-attention-soft px-4 py-2.5 text-sm text-attention-ink" role="status" data-testid="reader-outcome">
				Readers get nothing for it now: it was taken down. Publish it again to bring it back.
			</p>
		{:else if notYet}
			<p class="rounded-lg bg-pending-soft px-4 py-2.5 text-sm text-pending-ink" role="status" data-testid="reader-outcome">
				Readers don't get it yet. Once it is published, it is here.
			</p>
		{:else if read.isError}
			<Problem error={read.error} what="what readers get" retry={() => read.refetch()} />
		{:else}
			<dl class="space-y-2 text-sm">
				<div class="flex items-baseline justify-between gap-4">
					<dt class="text-muted-foreground" data-testid="reader-title-label">{titleLabel}</dt>
					<dd class="min-w-0 truncate font-medium" data-user-content data-testid="reader-title">{read.data.data.title}</dd>
				</div>
				{#if read.data.data.slug}
					<div class="flex items-baseline justify-between gap-4">
						<dt class="text-muted-foreground">{capital(URL_NAME)}</dt>
						<dd class="min-w-0 truncate font-mono text-xs" data-user-content>{read.data.data.slug}</dd>
					</div>
				{/if}
				<div class="flex items-baseline justify-between gap-4">
					<dt class="text-muted-foreground">This version went live in</dt>
					<dd class="tabular" data-testid="reader-release">{releaseName(read.data.data.published_in)}</dd>
				</div>
			</dl>
			{#if changed}
				<p class="rounded-lg bg-pending-soft px-4 py-2.5 text-sm text-pending-ink" data-testid="reader-changed">
					Your newer changes aren't in it: readers get them once you publish.
				</p>
			{/if}
			<div class="grid gap-1.5">
				<p class="text-xs text-muted-foreground">Exactly as your website receives it:</p>
				<!-- The API's own field names, as sent: technical, so the plain-words check leaves it be. -->
				<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
				<!-- A scrollable region must be reachable by keyboard (WCAG 2.1.1; axe scrollable-region-focusable). -->
				<pre
					class="max-h-[50vh] overflow-auto rounded-xl border border-border bg-muted/40 p-4 font-mono text-xs leading-relaxed"
					tabindex="0"
					aria-label="The data your website receives"
					data-technical
					data-testid="reader-data">{pretty}</pre>
			</div>
		{/if}
		{#if read.data && !read.isError}
			<Dialog.Footer>
				<Button onclick={copy} data-testid="reader-copy"><CopyIcon data-icon="inline-start" />Copy</Button>
			</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>
