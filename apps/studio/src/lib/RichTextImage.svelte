<!--
  An image inside rich text, as the editor draws it (gap 3 D1): the real picture from the media
  origin (its `w320`/`w1280` renditions when the worker wrote them), the upload's state while it
  isn't ready — Checking… in quarantine, Rejected, Deleted, Missing when no upload of this
  project has the id — and its alt text, with Edit alt text, Replace and Remove. The node's stored
  JSON stays `{assetId, alt}`: the buttons ask the toolbar's dialog (an `colophon-image` event) or
  delete the node; nothing here writes attributes.
  Mounted by `imageView` outside the component tree, so it reads the upload through its own
  query observer on the Studio's query client; the worker's `assets` hints refresh it like any
  other read, so Checking… turns into the picture without a reload.
-->
<script lang="ts">
	import ImageIcon from '@lucide/svelte/icons/image';
	import ImageOffIcon from '@lucide/svelte/icons/image-off';
	import { QueryObserver, type QueryClient, type QueryObserverResult } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { ApiError, getAsset, type Asset } from './api.ts';
	import { stateText, thumbnailSrcset, thumbnailUrl } from './media.ts';

	let {
		project,
		queryClient,
		assetId,
		alt,
		selected = false,
		editable = true,
		act,
		remove
	}: {
		project: string;
		queryClient: QueryClient;
		assetId: string;
		alt: string;
		selected?: boolean;
		editable?: boolean;
		act: (action: 'edit' | 'replace') => void;
		remove: () => void;
	} = $props();

	let result = $state.raw<QueryObserverResult<Asset, Error> | null>(null);
	$effect(() => {
		const observer = new QueryObserver<Asset, Error>(queryClient, {
			queryKey: ['asset', project, assetId],
			queryFn: () => getAsset(project, assetId),
			staleTime: 5_000,
			retry: false
		});
		result = observer.getCurrentResult();
		const off = observer.subscribe((next) => (result = next));
		return off;
	});

	const asset = $derived(result?.data);
	const missing = $derived(result?.error instanceof ApiError && (result.error.failure === 'not-found' || result.error.status === 404));
	/** One word for the tests and the styles: what the picture slot shows. */
	const status = $derived(
		asset !== undefined ? asset.state : missing ? 'missing' : result?.isError ? 'unknown' : 'loading'
	);
</script>

<figure class="m-0 grid gap-2 rounded-lg border bg-card p-2 font-sans {selected ? 'border-ring ring-[3px] ring-ring/40' : 'border-border'}" data-testid="rich-image" data-state={status} data-asset-id={assetId}>
	<div class="flex min-h-24 items-center justify-center overflow-hidden rounded-md bg-muted">
		{#if asset?.state === 'usable'}
			<img
				src={thumbnailUrl(project, asset)}
				srcset={thumbnailSrcset(project, asset) || undefined}
				sizes="(min-width: 640px) 24rem, 100vw"
				width={asset.width ?? undefined}
				height={asset.height ?? undefined}
				{alt}
				decoding="async"
				class="m-0! h-auto max-h-72 w-auto max-w-full rounded-none! object-contain"
				draggable="false"
			/>
		{:else if status === 'loading'}
			<div class="h-40 w-full animate-pulse bg-muted motion-reduce:animate-none" aria-hidden="true"></div>
		{:else}
			<div class="flex h-40 w-full flex-col items-center justify-center gap-1.5 px-4 text-center text-xs text-muted-foreground">
				{#if status === 'missing' || status === 'rejected' || status === 'deleted' || status === 'deleting'}
					<ImageOffIcon class="size-5" aria-hidden="true" />
				{:else}
					<ImageIcon class="size-5" aria-hidden="true" />
				{/if}
				{#if status === 'quarantined'}
					<span>This upload is being checked. It shows here once it is ready.</span>
				{:else if status === 'rejected'}
					<span>This upload was rejected{asset?.rejected_reason ? `: ${asset.rejected_reason}` : ''}. Replace it before publishing.</span>
				{:else if status === 'deleting' || status === 'deleted'}
					<span>This upload was deleted. Replace it or remove it.</span>
				{:else if status === 'missing'}
					<span class="text-destructive">This image isn't in the media library. Choose a replacement or remove it.</span>
				{:else}
					<span>Couldn't load this image's details just now.</span>
				{/if}
			</div>
		{/if}
	</div>
	<figcaption class="flex flex-wrap items-center gap-x-2 gap-y-1.5 text-start text-xs">
		{#if status === 'quarantined'}
			<Badge variant="pending" data-testid="rich-image-state">Checking…</Badge>
		{:else if status === 'rejected' || status === 'deleting' || status === 'deleted'}
			<Badge variant="destructive" data-testid="rich-image-state">{stateText(status)}</Badge>
		{:else if status === 'missing'}
			<Badge variant="destructive" data-testid="rich-image-state">Missing</Badge>
		{/if}
		<span class="min-w-0 flex-1 text-muted-foreground">
			<span class="font-medium text-foreground">Alt text:</span>
			<span data-user-content data-testid="rich-image-alt">{alt}</span>
		</span>
		{#if editable}
			<span class="flex flex-wrap gap-1">
				<Button variant="outline" size="xs" onclick={() => act('edit')} data-testid="rich-image-edit">Edit alt text</Button>
				<Button variant="outline" size="xs" onclick={() => act('replace')} data-testid="rich-image-replace">Replace</Button>
				<Button variant="ghost" size="xs" onclick={remove} data-testid="rich-image-remove">Remove</Button>
			</span>
			{#if selected}
				<span class="basis-full text-muted-foreground">Press Enter to edit the alt text, or Delete to remove the image.</span>
			{/if}
		{/if}
	</figcaption>
</figure>
