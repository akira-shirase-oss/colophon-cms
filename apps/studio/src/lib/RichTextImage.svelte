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

<!-- The picture at its own shape, up to the column's width (D-184): no grey bars beside it. Its
     actions float over its corner while it is pointed at, focused or selected (and always on a
     touch screen, which can't point); the alt text sits under it as a quiet caption. -->
<figure
	class="m-0 grid max-w-full gap-1.5 font-sans {asset?.state === 'usable' ? 'w-fit' : 'w-full'}"
	data-testid="rich-image"
	data-state={status}
	data-asset-id={assetId}
	data-selected={selected || undefined}
>
	<div class="relative min-w-0 rounded-lg outline-offset-2 {selected ? 'outline-2 outline-ring' : ''}">
		{#if asset?.state === 'usable'}
			<img
				src={thumbnailUrl(project, asset)}
				srcset={thumbnailSrcset(project, asset) || undefined}
				sizes="(min-width: 640px) 44rem, 100vw"
				width={asset.width ?? undefined}
				height={asset.height ?? undefined}
				{alt}
				decoding="async"
				class="m-0! block h-auto max-h-[36rem] w-auto max-w-full rounded-lg! object-contain"
				draggable="false"
			/>
		{:else if status === 'loading'}
			<div class="h-40 w-full max-w-md animate-pulse rounded-lg bg-muted motion-reduce:animate-none" aria-hidden="true"></div>
		{:else}
			<div class="flex h-40 w-full max-w-md flex-col items-center justify-center gap-1.5 rounded-lg border border-dashed border-border bg-card px-4 text-center text-sm text-muted-foreground">
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
		{#if editable}
			<span
				class="actions absolute top-2 right-2 flex gap-0.5 rounded-control border border-border bg-popover p-0.5 shadow-float transition-opacity"
				data-testid="rich-image-actions"
			>
				<Button variant="ghost" size="xs" onclick={() => act('edit')} title="Edit alt text (Enter when the image is selected)" data-testid="rich-image-edit">Edit alt text</Button>
				<Button variant="ghost" size="xs" onclick={() => act('replace')} data-testid="rich-image-replace">Replace</Button>
				<Button variant="ghost" size="xs" onclick={remove} title="Remove the image (Delete when it is selected)" data-testid="rich-image-remove">Remove</Button>
			</span>
		{/if}
	</div>
	<figcaption class="flex max-w-prose flex-wrap items-center gap-x-2 gap-y-1 text-start text-sm text-muted-foreground">
		{#if status === 'quarantined'}
			<Badge variant="pending" data-testid="rich-image-state">Checking…</Badge>
		{:else if status === 'rejected' || status === 'deleting' || status === 'deleted'}
			<Badge variant="destructive" data-testid="rich-image-state">{stateText(status)}</Badge>
		{:else if status === 'missing'}
			<Badge variant="destructive" data-testid="rich-image-state">Missing</Badge>
		{/if}
		<span class="min-w-0 flex-1 text-pretty" title="Alt text: what people who can't see the image hear">
			<span class="sr-only">Alt text: </span><span data-user-content data-testid="rich-image-alt">{alt}</span>
		</span>
	</figcaption>
</figure>

<style>
	/* Pointer screens show the actions on hover, focus or selection; touch screens always. */
	@media (hover: hover) {
		.actions {
			opacity: 0;
		}
		figure:hover .actions,
		figure:focus-within .actions,
		figure[data-selected] .actions {
			opacity: 1;
		}
	}
</style>
