<!--
  The project's media library as a chooser (media M.2, D-095; gap 3 D1), shared by asset fields
  and the rich-text "Insert an image" dialog: search (by name, title or alt text), a page at a
  time over the server's cursor, upload inside the panel, and the upload states in plain words.
  Usable uploads show their `w320` picture; anything still in quarantine or rejected shows its
  state, because a draft may name either (only publishing needs usable). An upload sent from here
  waits on the library — refreshed by the worker's `assets` hints (D-108) — and a usable result is
  chosen at once.
-->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ImageIcon from '@lucide/svelte/icons/image';
	import SearchIcon from '@lucide/svelte/icons/search';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import { createInfiniteQuery, type InfiniteData, useQueryClient } from '@tanstack/svelte-query';
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, deleteAsset, listAssetPage, newKey, uploadAsset, type Asset, type AssetPage } from './api.ts';
	import { describedAs, pickable, stateText, thumbnailUrl } from './media.ts';

	let {
		project,
		chosen = [],
		onpick,
		allowDelete = false,
		label = 'Uploads',
		autofocus = false
	}: {
		project: string;
		/** Uploads already chosen: marked, and not offered again. */
		chosen?: string[];
		/** Called with the upload a person chose, or with a fresh upload once it is usable. */
		onpick: (asset: Asset) => void;
		/** Offers Delete on each usable or rejected upload (the asset field's library does). */
		allowDelete?: boolean;
		/** The list's accessible name. */
		label?: string;
		/** Puts the cursor in the search box when the panel opens (inside a dialog). */
		autofocus?: boolean;
	} = $props();

	/** Uploads per page: enough to fill the grid twice, small enough to come back at once. */
	const PAGE = 30;
	const queryClient = useQueryClient();
	const id = $props.id();

	// The search box, and what the server is asked for 250 ms after typing stops.
	let search = $state('');
	let q = $state('');
	$effect(() => {
		const typed = search.trim();
		const timer = setTimeout(() => (q = typed), 250);
		return () => clearTimeout(timer);
	});

	const key = $derived(['assets', project, 'library', q]);
	const library = createInfiniteQuery(() => ({
		queryKey: key,
		queryFn: ({ pageParam }: { pageParam: string | null }) => listAssetPage(project, { q: q || undefined, cursor: pageParam ?? undefined, limit: PAGE }),
		initialPageParam: null as string | null,
		getNextPageParam: (last: AssetPage) => last.next_cursor,
		staleTime: 5_000
	}));
	const visible = $derived((library.data?.pages ?? []).flatMap((page) => page.data));

	let fileInput = $state<HTMLInputElement>();
	let searchInput = $state<HTMLInputElement | null>(null);
	let uploading = $state('');
	let problem = $state('');

	$effect(() => {
		if (autofocus) searchInput?.focus();
	});

	// Deleting from the library: a confirmed 202 is requested, not gone — the tile shows
	// Deleting…, then Deleted, until the list no longer names it. A refusal's message, with who
	// still names it, shows where upload problems do.
	let confirming = $state<Asset | null>(null);
	let deleting = $state(false);

	/** Asks for `confirming`'s asset to go, then refreshes the library to show its state. */
	async function remove() {
		const asset = confirming;
		if (asset === null || deleting) return;
		problem = '';
		deleting = true;
		try {
			await deleteAsset(project, asset.id, newKey());
			confirming = null;
			await library.refetch();
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

	/** Upload `id` as the loaded pages show it, if they do. */
	function seen(upload: string): Asset | undefined {
		const data = queryClient.getQueryData<InfiniteData<AssetPage>>(['assets', project, 'library', '']);
		return (data?.pages ?? []).flatMap((page) => page.data).find((asset) => asset.id === upload);
	}

	/** Waits for upload `id` to leave quarantine, watching the library's reads: the worker's
	 *  commit arrives as an `assets` hint (D-108) which refreshes it — the 10-second timer is only
	 *  a backstop for the stream being down, and only then does this read again itself. `null`
	 *  when it hasn't settled within the half minute. */
	async function settled(upload: string): Promise<Asset | null> {
		const deadline = Date.now() + 30_000;
		while (Date.now() < deadline) {
			const found = seen(upload);
			if (found !== undefined && found.state !== 'quarantined') return found;
			const heard = await new Promise<boolean>((resolve) => {
				const timer = setTimeout(() => resolve(false), Math.min(10_000, Math.max(1, deadline - Date.now())));
				const off = queryClient.getQueryCache().subscribe((event) => {
					if (event.type === 'updated' && event.query.queryKey[0] === 'assets' && event.query.queryKey[1] === project) {
						clearTimeout(timer);
						off();
						resolve(true);
					}
				});
			});
			if (!heard) await library.refetch();
		}
		return null;
	}

	/** Sends the picked file into quarantine and waits for the worker's verdict; a usable result
	 *  is chosen at once. The search is cleared first, so the new upload is the first tile. */
	async function upload(file: File) {
		problem = '';
		uploading = file.name;
		search = '';
		q = '';
		try {
			const made = await uploadAsset(project, file);
			await library.refetch();
			// Quarantine settles on the worker: wait on the library's reads (live-refreshed), not on
			// polling this one record.
			const asset = made.state === 'quarantined' ? ((await settled(made.id)) ?? made) : made;
			if (asset.state === 'usable') {
				onpick(asset);
			} else if (asset.state === 'rejected') {
				problem = `“${file.name}” was rejected: ${asset.rejected_reason ?? 'not a usable image'}`;
			} else {
				problem = `“${file.name}” is still being checked. It will show here, ready to choose, in a moment.`;
			}
		} catch (error) {
			problem = error instanceof ApiError ? `Couldn't upload “${file.name}”: ${error.message}` : `Couldn't upload “${file.name}”.`;
		} finally {
			uploading = '';
			if (fileInput) fileInput.value = '';
		}
	}
</script>

<div class="grid gap-3" data-testid="asset-library">
	<div class="flex flex-wrap items-center gap-2">
		<div class="relative min-w-0 flex-1 basis-48">
			<SearchIcon class="pointer-events-none absolute start-2.5 top-1/2 size-4 -translate-y-1/2 text-muted-foreground" aria-hidden="true" />
			<label for="{id}-search" class="sr-only">Search uploads</label>
			<Input
				id="{id}-search"
				type="search"
				bind:ref={searchInput}
				bind:value={search}
				maxlength={100}
				placeholder="Search by name, title or alt text"
				class="ps-8"
				data-testid="asset-search"
			/>
		</div>
		<Button variant="outline" size="sm" disabled={uploading !== ''} onclick={() => fileInput?.click()} data-testid="asset-upload">
			<UploadIcon aria-hidden="true" />{uploading === '' ? 'Upload an image' : `Checking ${uploading}…`}
		</Button>
		<input
			bind:this={fileInput}
			type="file"
			accept="image/png,image/jpeg,image/webp"
			class="hidden"
			onchange={(event) => {
				const file = event.currentTarget.files?.[0];
				if (file) upload(file);
			}}
			data-testid="asset-file"
		/>
	</div>
	{#if problem}<p class="text-xs text-destructive" role="alert" data-testid="asset-problem">{problem}</p>{/if}

	{#if library.isPending}
		<ul class="grid grid-cols-2 gap-2 sm:grid-cols-3" aria-label="Loading the library" aria-busy="true" data-testid="asset-loading">
			{#each { length: 6 } as _, index (index)}
				<li class="overflow-hidden rounded-lg border border-border">
					<Skeleton class="aspect-video w-full rounded-none" />
					<div class="px-2 py-1.5"><Skeleton class="h-4 w-2/3" /></div>
				</li>
			{/each}
		</ul>
		<p class="sr-only" role="status">Loading the library…</p>
	{:else if library.isError}
		<div class="flex flex-wrap items-center gap-2 rounded-lg border border-destructive/30 bg-destructive/5 px-3 py-2" role="alert">
			<p class="text-xs text-destructive">Couldn't list the uploads just now.</p>
			<Button variant="outline" size="xs" onclick={() => library.refetch()}>Try again</Button>
		</div>
	{:else if visible.length === 0 && q !== ''}
		<div class="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border px-4 py-8 text-center" data-testid="asset-no-match">
			<p class="text-sm">No uploads match “<span data-user-content>{q}</span>”.</p>
			<p class="text-xs text-muted-foreground">Search looks at file names, titles and alt text.</p>
			<Button variant="outline" size="sm" onclick={() => ((search = ''), (q = ''), searchInput?.focus())}>Clear the search</Button>
		</div>
	{:else if visible.length === 0}
		<div class="flex flex-col items-center gap-2 rounded-lg border border-dashed border-border px-4 py-8 text-center">
			<ImageIcon class="size-5 text-muted-foreground" aria-hidden="true" />
			<p class="text-sm">No uploads yet — upload an image to choose it.</p>
			<p class="text-xs text-muted-foreground">PNG, JPEG or WebP, up to 20 MB. Each upload is checked before it can be published.</p>
		</div>
	{:else}
		<div class="max-h-[26rem] overflow-y-auto p-0.5">
			<ul class="grid grid-cols-2 gap-2 sm:grid-cols-3" aria-label={label}>
				{#each visible as asset (asset.id)}
					{@const isChosen = chosen.includes(asset.id)}
					<li class="group relative">
						<button
							type="button"
							class="w-full overflow-hidden rounded-lg border text-left outline-none enabled:hover:border-primary focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/50 disabled:opacity-60 {isChosen ? 'border-primary' : 'border-border'}"
							disabled={isChosen || !pickable(asset)}
							onclick={() => onpick(asset)}
							data-testid="asset-option"
							data-state={asset.state}
						>
							{#if asset.state === 'usable'}
								<img src={thumbnailUrl(project, asset)} alt={describedAs(asset)} loading="lazy" decoding="async" class="aspect-video w-full bg-muted object-cover" />
							{:else}
								<div class="flex aspect-video w-full items-center justify-center bg-muted text-xs {asset.state === 'rejected' ? 'text-destructive' : 'text-muted-foreground'}">
									{stateText(asset.state)}
								</div>
							{/if}
							<span class="flex items-center gap-1 px-2 py-1.5 text-xs">
								{#if isChosen}<CheckIcon class="size-3.5 shrink-0 text-primary" aria-label="Chosen" />{/if}
								<span class="min-w-0 flex-1 truncate" data-user-content>{asset.file_name}</span>
							</span>
						</button>
						{#if allowDelete && (asset.state === 'usable' || asset.state === 'rejected')}
							<button
								type="button"
								class="absolute end-1 top-1 rounded-md bg-card/90 p-1 text-muted-foreground opacity-0 outline-none transition-opacity hover:text-destructive focus-visible:opacity-100 focus-visible:ring-[3px] focus-visible:ring-ring/50 group-hover:opacity-100"
								aria-label="Delete {asset.file_name}"
								onclick={() => (confirming = asset)}
								data-testid="asset-delete"
							>
								<TrashIcon class="size-3.5" />
							</button>
						{/if}
					</li>
				{/each}
			</ul>
			{#if library.hasNextPage || library.isFetchNextPageError}
				<div class="mt-3 flex flex-wrap items-center justify-center gap-2">
					{#if library.isFetchNextPageError}<p class="text-xs text-destructive" role="alert">Couldn't load more just now.</p>{/if}
					<Button variant="outline" size="sm" disabled={library.isFetchingNextPage} onclick={() => library.fetchNextPage()} data-testid="asset-more">
						{library.isFetchingNextPage ? 'Loading more…' : library.isFetchNextPageError ? 'Try again' : 'Load more uploads'}
					</Button>
				</div>
			{/if}
		</div>
	{/if}
</div>

{#if allowDelete}
	<AlertDialog.Root open={confirming !== null} onOpenChange={(next) => !next && (confirming = null)}>
		<AlertDialog.Content>
			<AlertDialog.Header>
				<AlertDialog.Title>Delete {confirming?.file_name ?? 'this upload'}?</AlertDialog.Title>
				<AlertDialog.Description>
					The file and its renditions go for good. If an entry or a release still names it, the
					server refuses instead — take it out of those first.
				</AlertDialog.Description>
			</AlertDialog.Header>
			<AlertDialog.Footer>
				<AlertDialog.Cancel disabled={deleting}>Keep it</AlertDialog.Cancel>
				<AlertDialog.Action variant="destructive" disabled={deleting} onclick={remove}>
					{deleting ? 'Deleting…' : 'Delete it'}
				</AlertDialog.Action>
			</AlertDialog.Footer>
		</AlertDialog.Content>
	</AlertDialog.Root>
{/if}
