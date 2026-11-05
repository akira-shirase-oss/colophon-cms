<!--
	The media library (M.2, D-095, D-136): every upload in the project, what state it settled in,
	and the two things an editor does with one — upload a file and delete a file nothing
	references. A fresh upload shows Checking… until the worker settles it; the project's events
	stream invalidates ['assets', project] as `assets` hints land, so the grid settles live
	without a reload.
-->
<script lang="ts">
	import ImageIcon from '@lucide/svelte/icons/image';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, assetUrl, deleteAsset, listAssets, newKey, uploadAsset, type Asset } from '#lib/api.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const assets = createQuery(() => ({ queryKey: ['assets', project], queryFn: () => listAssets(project), staleTime: 5_000 }));
	const visible = $derived(assets.data ?? []);

	let fileInput = $state<HTMLInputElement>();
	let uploading = $state(false);
	let problem = $state('');
	let confirming = $state<Asset | null>(null);
	let deleting = $state(false);

	async function upload(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		if (file === undefined) return;
		problem = '';
		uploading = true;
		try {
			await uploadAsset(project, file);
			await queryClient.invalidateQueries({ queryKey: ['assets', project] });
		} catch (error) {
			problem = error instanceof ApiError ? error.message : `Couldn't upload “${file.name}”.`;
		} finally {
			uploading = false;
			input.value = '';
		}
	}

	/** Asks for `confirming`'s asset to go, then refreshes the grid to show its state. */
	async function remove() {
		const asset = confirming;
		if (asset === null || deleting) return;
		problem = '';
		deleting = true;
		try {
			await deleteAsset(project, asset.id, newKey());
			confirming = null;
			await assets.refetch();
		} catch (error) {
			confirming = null;
			problem =
				error instanceof ApiError
					? error.details?.referenced_by !== undefined && error.details.referenced_by.length > 0
						? `${error.message} (${error.details.referenced_by
								.slice(0, 3)
								.map((referrer) => referrer.collection)
								.join(', ')}${error.details.referenced_by.length > 3 ? ', …' : ''})`
						: error.message
					: `Couldn't delete “${asset.file_name}”.`;
		} finally {
			deleting = false;
		}
	}

	/** How an upload's record shows beside its name. */
	function stateText(state: string): string {
		switch (state) {
			case 'usable':
				return 'Usable';
			case 'rejected':
				return 'Rejected';
			case 'deleting':
				return 'Deleting…';
			case 'deleted':
				return 'Deleted';
			default:
				return 'Checking…';
		}
	}

	/** Only settled states can be asked to go; a verifying upload waits for its verdict. */
	const deletable = (asset: Asset) => asset.state === 'usable' || asset.state === 'rejected';

	function size(bytes: number): string {
		if (bytes < 1024) return `${bytes} B`;
		if (bytes < 1024 * 1024) return `${Math.round(bytes / 1024)} KB`;
		return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
	}
</script>

<PageHeader title="Media">
	{#snippet description()}
		Every upload in this project: what it settled in, how big it is, and where it came from. Uploads are checked before they can be used; a
		rejected file never reaches a page.
	{/snippet}
	{#snippet actions()}
		<Button onclick={() => fileInput?.click()} disabled={uploading} data-testid="upload-asset">
			<UploadIcon data-icon="inline-start" />{uploading ? 'Uploading…' : 'Upload'}
		</Button>
		<input bind:this={fileInput} onchange={upload} type="file" accept="image/*" class="sr-only" data-testid="asset-file" />
	{/snippet}
</PageHeader>

{#if problem !== ''}
	<p class="mb-4 rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm" role="alert" data-testid="media-problem">
		{problem}
	</p>
{/if}

{#if assets.isPending}
	<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4">
		{#each [0, 1, 2, 3] as index (index)}<Skeleton class="h-56 w-full rounded-xl" />{/each}
	</div>
{:else if assets.isError}
	<Problem error={assets.error} what="media" retry={() => assets.refetch()} />
{:else if visible.length === 0}
	<EmptyState icon={ImageIcon} title="No media yet" testid="media-empty">
		Upload an image and it lands here — checked, measured, and ready to pick from any entry's image field.
	</EmptyState>
{:else}
	<ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4" data-testid="media-grid">
		{#each visible as asset (asset.id)}
			<li class="flex flex-col overflow-hidden rounded-xl border border-border bg-card shadow-xs">
				<div class="flex h-40 items-center justify-center overflow-hidden border-b border-border bg-secondary/40">
					{#if asset.state === 'usable'}
						<img src={assetUrl(project, asset.id)} alt={asset.file_name} class="h-full w-full object-cover" loading="lazy" />
					{:else}
						<ImageIcon class="size-8 text-muted-foreground" aria-hidden="true" />
					{/if}
				</div>
				<div class="flex min-w-0 flex-1 flex-col gap-2 p-3">
					<div class="flex items-start justify-between gap-2">
						<span class="min-w-0 truncate font-medium" title={asset.file_name} data-user-content>{asset.file_name}</span>
						<Badge variant={asset.state === 'rejected' ? 'destructive' : 'secondary'} class="shrink-0" data-testid="asset-state">
							{stateText(asset.state)}
						</Badge>
					</div>
					<p class="text-sm text-muted-foreground">
						{#if asset.width !== null && asset.height !== null}
							{asset.width}×{asset.height} · {size(asset.size_bytes)}
						{:else}
							{size(asset.size_bytes)}
						{/if}
					</p>
					{#if asset.rejected_reason !== null && asset.rejected_reason !== ''}
						<p class="text-sm text-destructive" data-testid="asset-reason">{asset.rejected_reason}</p>
					{/if}
					<div class="mt-auto flex justify-end">
						<Button
							variant="ghost"
							size="sm"
							disabled={!deletable(asset)}
							onclick={() => (confirming = asset)}
							data-testid="delete-asset"
						>
							<TrashIcon data-icon="inline-start" />Delete
						</Button>
					</div>
				</div>
			</li>
		{/each}
	</ul>
{/if}

<Dialog.Root open={confirming !== null} onOpenChange={(open) => !open && (confirming = null)}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Delete “{confirming?.file_name}”?</Dialog.Title>
			<Dialog.Description>
				The file leaves the library in stages: it stops being served at once and the stored bytes go shortly after. An entry that still names it
				will refuse the delete and say which.
			</Dialog.Description>
		</Dialog.Header>
		<div class="mt-4 flex justify-end gap-2">
			<Button variant="outline" onclick={() => (confirming = null)} disabled={deleting}>Keep it</Button>
			<Button variant="destructive" onclick={remove} disabled={deleting} data-testid="confirm-delete">
				{deleting ? 'Deleting…' : 'Delete'}
			</Button>
		</div>
	</Dialog.Content>
</Dialog.Root>
