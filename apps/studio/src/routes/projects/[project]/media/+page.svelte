<!--
	The media library (M.2, D-095, D-136, D2): every upload in the project, newest first, read a
	page at a time over the server's cursor — a search (file name, title, alt text), a state and a
	tag narrow it on the server, so nothing past the first page is out of reach. Thumbnails come
	from the 320-pixel rendition. Files go in by the Upload button or by dropping them anywhere on
	the page, through a queue that sends three at a time and says how each one ended. A card
	opens its details: the library's words for it, versions, where it's used, replace, addresses,
	delete. The project's events stream invalidates ['assets', project] as `assets` hints land, so
	the grid settles live without a reload.
-->
<script lang="ts">
	import ImageIcon from '@lucide/svelte/icons/image';
	import SearchIcon from '@lucide/svelte/icons/search';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import UploadIcon from '@lucide/svelte/icons/upload';
	import XIcon from '@lucide/svelte/icons/x';
	import { page } from '$app/state';
	import { createInfiniteQuery, createQuery, keepPreviousData, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import * as InputGroup from '@colophon/ui/ui/input-group';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, type Asset, type AssetFilter, type AssetPage, deleteAsset, listAssetPage, may, newKey } from '#lib/api.ts';
	import { altOf, carriesFiles, deletable, MAX_CARDS, MAX_QUERY, measure, PAGE_SIZE, sentence, stateText, TAG, thumbnailUrl } from '#lib/library.ts';
	import MediaDetails from '#lib/MediaDetails.svelte';
	import { projectQuery } from '#lib/queries.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import UploadQueue from '#lib/UploadQueue.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	// What the role may do here; while the project loads, the buttons show and the server decides.
	const uploads = $derived(mine.data === undefined || may(mine.data, 'content.create'));
	const edits = $derived(mine.data === undefined || may(mine.data, 'content.update'));
	const deletes = $derived(mine.data === undefined || may(mine.data, 'content.delete'));

	// The search, typed then settled for 250 ms; the state and the tag apply at once.
	let typed = $state('');
	let q = $state('');
	$effect(() => {
		const next = typed;
		const timer = setTimeout(() => (q = next.trim().slice(0, MAX_QUERY)), 250);
		return () => clearTimeout(timer);
	});
	type StateFilter = '' | NonNullable<AssetFilter['state']>;
	let shown = $state<StateFilter>('');
	let tag = $state(page.url.searchParams.get('tag') ?? '');
	const narrowed = $derived(q !== '' || shown !== '' || tag !== '');

	const library = createInfiniteQuery(() => ({
		queryKey: ['assets', project, 'library', q, shown, tag],
		queryFn: ({ pageParam }: { pageParam: string | null }) =>
			listAssetPage(project, {
				q,
				...(shown === '' ? {} : { state: shown }),
				...(tag === '' || !TAG.test(tag) ? {} : { tag }),
				limit: PAGE_SIZE,
				...(pageParam === null ? {} : { cursor: pageParam })
			}),
		initialPageParam: null as string | null,
		getNextPageParam: (last: AssetPage) => last.next_cursor,
		placeholderData: keepPreviousData,
		staleTime: 5_000
	}));
	const loaded = $derived((library.data?.pages ?? []).flatMap((one) => one.data));
	// A server that predates paging answers every upload at once and no cursor: the same page.
	const more = $derived(library.hasNextPage && loaded.length < MAX_CARDS);
	const capped = $derived(library.hasNextPage && loaded.length >= MAX_CARDS);

	function loadMore() {
		if (more && !library.isFetchingNextPage) void library.fetchNextPage();
	}

	// Scrolling near the end reads the next page; the button stays for keyboards and for anyone
	// who'd rather ask.
	let sentinel = $state<HTMLElement | null>(null);
	$effect(() => {
		// Unset (null) whenever the grid isn't showing: nothing to watch.
		const target = sentinel;
		if (target === null || typeof IntersectionObserver === 'undefined') return;
		const observer = new IntersectionObserver((seen) => {
			if (seen.some((entry) => entry.isIntersecting)) loadMore();
		}, { rootMargin: '400px 0px' });
		observer.observe(target);
		return () => observer.disconnect();
	});

	function clearFilters() {
		typed = '';
		q = '';
		shown = '';
		tag = '';
	}

	function filterByTag(chosen: string) {
		tag = chosen;
		open = false;
	}

	// Uploads: the button's picker and a drop anywhere on the page feed one queue.
	let queue = $state<ReturnType<typeof UploadQueue>>();
	let fileInput = $state<HTMLInputElement>();
	function picked(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		queue?.add(Array.from(input.files ?? []));
		input.value = '';
	}
	let dragging = $state(0);
	function dragEnter(event: DragEvent) {
		if (!uploads || !carriesFiles(event)) return;
		event.preventDefault();
		dragging += 1;
	}
	function dragOver(event: DragEvent) {
		if (!uploads || !carriesFiles(event)) return;
		event.preventDefault();
		if (event.dataTransfer) event.dataTransfer.dropEffect = 'copy';
	}
	function dragLeave(event: DragEvent) {
		if (!carriesFiles(event)) return;
		dragging = Math.max(0, dragging - 1);
	}
	function drop(event: DragEvent) {
		if (!uploads || !carriesFiles(event)) return;
		event.preventDefault();
		dragging = 0;
		queue?.add(Array.from(event.dataTransfer?.files ?? []));
	}

	// The details sheet.
	let opened = $state<Asset | null>(null);
	let open = $state(false);
	function show(asset: Asset) {
		opened = asset;
		open = true;
	}
	// The grid's newer read of the open upload, so the sheet's header follows a live change.
	const openedNow = $derived(opened === null ? null : (loaded.find((asset) => asset.id === opened?.id) ?? opened));

	// Deleting: confirmed, then 202 is requested, not gone — the card shows Deleting…, then Deleted.
	let problem = $state('');
	let confirming = $state<Asset | null>(null);
	let deleting = $state(false);
	function askDelete(asset: Asset) {
		open = false;
		confirming = asset;
	}
	async function remove() {
		const asset = confirming;
		if (asset === null || deleting) return;
		problem = '';
		deleting = true;
		try {
			await deleteAsset(project, asset.id, newKey());
			confirming = null;
			await queryClient.invalidateQueries({ queryKey: ['assets', project] });
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

	const badge = (state: string) => (state === 'usable' ? 'live' : state === 'rejected' ? 'attention' : state === 'quarantined' ? 'pending' : 'secondary');
	const STATES: { value: StateFilter; label: string }[] = [
		{ value: '', label: 'Every state' },
		{ value: 'usable', label: 'Ready' },
		{ value: 'quarantined', label: 'Being checked' },
		{ value: 'rejected', label: 'Rejected' },
		{ value: 'deleted', label: 'Deleted' }
	];
	const select =
		'h-9 w-full rounded-md border border-input bg-popover px-3 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40 sm:w-44';
</script>

<svelte:head><title>Media · Colophon Studio</title></svelte:head>

<div
	class="relative min-h-[60vh]"
	role="region"
	aria-label="Media library"
	ondragenter={dragEnter}
	ondragover={dragOver}
	ondragleave={dragLeave}
	ondrop={drop}
	data-testid="media-page"
>
	{#if dragging > 0}
		<div
			class="pointer-events-none absolute inset-0 z-20 flex items-center justify-center rounded-2xl border-2 border-dashed border-ring bg-background/85"
			data-testid="media-drop"
		>
			<p class="flex items-center gap-2 text-base font-medium"><UploadIcon class="size-5" aria-hidden="true" />Drop images to upload them</p>
		</div>
	{/if}

	<PageHeader title="Media">
		{#snippet description()}
			Every upload in this project. Files are checked before they can be used, and a rejected file never reaches a page. Drop images anywhere
			here to upload them.
		{/snippet}
		{#snippet actions()}
			{#if uploads}
				<Button onclick={() => fileInput?.click()} data-testid="upload-asset">
					<UploadIcon data-icon="inline-start" />Upload
				</Button>
				<input
					bind:this={fileInput}
					onchange={picked}
					type="file"
					accept="image/png,image/jpeg,image/webp"
					multiple
					class="sr-only"
					tabindex="-1"
					aria-hidden="true"
					data-testid="asset-file"
				/>
			{/if}
		{/snippet}
	</PageHeader>

	<UploadQueue bind:this={queue} {project} />

	{#if problem !== ''}
		<p class="mb-4 rounded-lg border border-destructive/40 bg-destructive/5 px-3 py-2 text-sm" role="alert" data-testid="media-problem">
			{problem}
		</p>
	{/if}

	<!-- Nothing to narrow in an empty library: the bar waits for the first upload. -->
	<div
		class="mb-5 flex flex-col gap-3 sm:flex-row sm:flex-wrap sm:items-center"
		class:hidden={!narrowed && !library.isPending && !library.isError && loaded.length === 0}
		role="search"
		aria-label="Narrow the library"
	>
		<InputGroup.Root class="sm:w-80">
			<InputGroup.Addon><SearchIcon /></InputGroup.Addon>
			<InputGroup.Input
				type="search"
				placeholder="Search names, titles and alt text"
				aria-label="Search the library"
				maxlength={MAX_QUERY}
				bind:value={typed}
				data-testid="media-search"
			/>
		</InputGroup.Root>
		<select class={select} bind:value={shown} aria-label="Show uploads in a state" data-testid="media-state">
			{#each STATES as option (option.value)}<option value={option.value}>{option.label}</option>{/each}
		</select>
		{#if tag !== ''}
			<Badge variant="outline" class="h-9 gap-1 rounded-full pr-1 pl-3 text-sm" data-testid="media-tag">
				Tagged #{tag}
				<Button variant="ghost" size="icon-sm" class="size-7 rounded-full" onclick={() => (tag = '')} aria-label="Stop showing only #{tag}"><XIcon /></Button>
			</Badge>
		{/if}
		<p class="text-sm text-muted-foreground sm:ml-auto" role="status" data-testid="media-count">
			{#if library.isFetching && !library.isFetchingNextPage && !library.isPending}
				Looking…
			{:else if !library.isPending && !library.isError && loaded.length > 0}
				{loaded.length === 1 ? '1 upload' : `${loaded.length} uploads`}{library.hasNextPage ? ' so far' : ''}{narrowed ? ' match' : ''}
			{/if}
		</p>
	</div>

	{#if library.isPending}
		<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4" aria-busy="true" aria-label="Loading the library">
			{#each [0, 1, 2, 3, 4, 5, 6, 7] as index (index)}
				<div class="overflow-hidden rounded-xl border border-border bg-card">
					<Skeleton class="h-40 w-full rounded-none" />
					<div class="grid gap-2 p-3"><Skeleton class="h-4 w-3/4" /><Skeleton class="h-3 w-1/2" /></div>
				</div>
			{/each}
		</div>
	{:else if library.isError}
		<Problem error={library.error} what="the media library" retry={() => library.refetch()} />
	{:else if loaded.length === 0 && narrowed}
		<EmptyState icon={SearchIcon} title={q !== '' ? `No uploads match “${q}”` : 'No uploads match these filters'} testid="media-no-match">
			Try other words, or show everything again.
			{#snippet action()}<Button variant="outline" onclick={clearFilters} data-testid="media-clear">Show everything</Button>{/snippet}
		</EmptyState>
	{:else if loaded.length === 0}
		<EmptyState icon={ImageIcon} title="No media yet" testid="media-empty">
			Upload images here, or drop them onto this page. Each one is checked, measured and resized, then it's ready to pick from any entry's
			image field or to put into rich text.
			{#snippet action()}
				{#if uploads}<Button variant="outline" onclick={() => fileInput?.click()} data-testid="media-empty-upload"><UploadIcon data-icon="inline-start" />Upload images</Button>{/if}
			{/snippet}
		</EmptyState>
	{:else}
		<ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-4" data-testid="media-grid" aria-label="Uploads">
			{#each loaded as asset (asset.id)}
				{@const thumb = thumbnailUrl(project, asset)}
				<li
					class="relative flex flex-col overflow-hidden rounded-xl border border-border bg-card transition-colors hover:border-foreground/20 has-[.open:focus-visible]:ring-[3px] has-[.open:focus-visible]:ring-ring/40"
					data-testid="media-card"
					data-state={asset.state}
				>
					<div class="flex h-40 items-center justify-center overflow-hidden border-b border-border bg-secondary/40">
						{#if thumb !== null}
							<img src={thumb} alt={altOf(asset)} class="h-full w-full object-cover" loading="lazy" decoding="async" data-testid="media-thumb" />
						{:else}
							<div class="flex flex-col items-center gap-1.5 text-muted-foreground">
								<ImageIcon class="size-7" aria-hidden="true" />
								<span class="text-xs">{asset.state === 'rejected' ? "Can't be shown" : stateText(asset.state)}</span>
							</div>
						{/if}
					</div>
					<div class="flex min-w-0 flex-1 flex-col gap-2 p-3">
						<div class="flex items-start justify-between gap-2">
							<button
								type="button"
								class="open min-w-0 truncate text-left font-medium outline-none after:absolute after:inset-0 after:content-['']"
								title={asset.file_name}
								onclick={() => show(asset)}
								aria-label="Details of {asset.file_name}"
								data-user-content
								data-testid="media-open">{asset.file_name}</button
							>
							<Badge variant={badge(asset.state)} class="shrink-0" data-testid="asset-state">{stateText(asset.state)}</Badge>
						</div>
						<p class="text-sm text-muted-foreground">
							{measure(asset)}
						</p>
						{#if asset.rejected_reason !== null && asset.rejected_reason !== ''}
							<p class="text-sm text-attention-ink" data-testid="asset-reason">{sentence(asset.rejected_reason)}</p>
						{/if}
						{#if asset.tags.length > 0}
							<div class="relative z-10 flex flex-wrap gap-1" role="group" aria-label="Tags">
								{#each asset.tags.slice(0, 3) as one (one)}
									<button
										type="button"
										class="rounded-full border border-border px-2 py-0.5 text-xs text-muted-foreground hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 focus-visible:outline-none"
										onclick={() => (tag = one)}
										aria-label="Show uploads tagged {one}">#{one}</button
									>
								{/each}
								{#if asset.tags.length > 3}<span class="px-1 text-xs text-muted-foreground">+{asset.tags.length - 3}</span>{/if}
							</div>
						{/if}
						{#if deletes}
							<div class="relative z-10 mt-auto flex justify-end">
								<Button variant="ghost" size="sm" disabled={!deletable(asset)} onclick={() => askDelete(asset)} aria-label="Delete {asset.file_name}" data-testid="delete-asset">
									<TrashIcon data-icon="inline-start" />Delete
								</Button>
							</div>
						{/if}
					</div>
				</li>
			{/each}
		</ul>
		<div bind:this={sentinel} class="h-px" aria-hidden="true"></div>
		<div class="mt-6 flex min-h-9 flex-col items-center gap-2">
			{#if more}
				<Button variant="outline" onclick={loadMore} disabled={library.isFetchingNextPage} data-testid="media-more">
					{library.isFetchingNextPage ? 'Loading more…' : 'Load more'}
				</Button>
			{:else if capped}
				<p class="text-sm text-muted-foreground" data-testid="media-capped">
					That's the newest {MAX_CARDS}. Search or filter to find older uploads.
				</p>
			{:else if library.isFetchNextPageError}
				<p class="text-sm text-destructive" role="alert">Couldn't load more. <Button variant="link" class="h-auto p-0" onclick={loadMore}>Try again</Button></p>
			{/if}
		</div>
	{/if}
</div>

<MediaDetails {project} asset={openedNow} bind:open {edits} {deletes} onfilter={filterByTag} ondelete={askDelete} />

<Dialog.Root open={confirming !== null} onOpenChange={(next) => !next && (confirming = null)}>
	<Dialog.Content class="sm:max-w-md">
		<Dialog.Header>
			<Dialog.Title>Delete “{confirming?.file_name}”?</Dialog.Title>
			<Dialog.Description>
				The file leaves the library in stages: it stops being served at once and the stored bytes go shortly after. An entry that still uses it
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
