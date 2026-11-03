<!--
  Choosing uploads for an `asset` field (media M.2, D-095): "Choose image" opens a panel over the
  project's uploads — usable images show their picture from the media origin, anything still in
  quarantine or rejected shows its state, because a draft may name either (only publishing needs
  usable). Upload inside the panel sends the file into quarantine and polls until the worker
  settles it; a usable result is picked at once. A chosen upload shows as its thumbnail and name
  with Change and Remove; a list shows chips that can be moved up and down and removed, up to its
  maximum, never the same upload twice. The server decides: an id that names no upload is refused,
  and marked on its chip.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ImageIcon from '@lucide/svelte/icons/image';
	import XIcon from '@lucide/svelte/icons/x';
	import { createQueries, createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { ApiError, assetUrl, getAsset, listAssets, uploadAsset, type Asset, type FieldDefinition } from './api.ts';

	let {
		project,
		field,
		value = $bindable(),
		disabled = false,
		input,
		describedBy,
		refusedAt = null,
		refusal = '',
		onchange = () => {}
	}: {
		project: string;
		field: Extract<FieldDefinition, { type: 'asset' }>;
		/** One id ('' for none), or the list of ids. */
		value: string | string[];
		disabled?: boolean;
		/** The library toggle's id, which the field's label names. */
		input: string;
		describedBy?: string;
		/** For a list: the place of the id the server refused. */
		refusedAt?: number | null;
		refusal?: string;
		onchange?: () => void;
	} = $props();

	const chosen = $derived(Array.isArray(value) ? value : value === '' ? [] : [value]);
	const full = $derived(field.many ? chosen.length >= field.max : chosen.length >= 1);

	// Each chosen upload's record: its name and state. A refused read is an upload the role
	// can't see (or one that isn't there).
	const records = createQueries(() => ({ queries: chosen.map((id) => ({ queryKey: ['asset', project, id], queryFn: () => getAsset(project, id), staleTime: 5_000, retry: false })) }));
	const named = $derived(chosen.map((id, index) => ({ id, asset: records[index]?.data, failed: records[index]?.isError === true })));
	const label = (asset: Asset | undefined, failed: boolean) => (failed ? 'An upload you can’t see' : asset === undefined ? 'Loading…' : asset.file_name);

	// The library: every upload in the project, shown while the panel is open.
	let open = $state(false);
	const library = createQuery(() => ({
		queryKey: ['assets', project],
		queryFn: () => listAssets(project),
		enabled: open,
		staleTime: 5_000
	}));
	const visible = $derived(library.data ?? []);

	let fileInput = $state<HTMLInputElement>();
	let uploading = $state('');
	let problem = $state('');

	/** Sends the picked file into quarantine and waits for the worker's verdict; a usable
	 *  result is chosen at once. */
	async function upload(file: File) {
		problem = '';
		uploading = file.name;
		try {
			const made = await uploadAsset(project, file);
			const deadline = Date.now() + 15_000;
			let asset = made;
			while (asset.state === 'quarantined') {
				if (Date.now() > deadline) break;
				await new Promise((wait) => setTimeout(wait, 300));
				asset = await getAsset(project, asset.id);
			}
			await library.refetch();
			if (asset.state === 'usable') {
				choose(asset.id);
			} else if (asset.state === 'rejected') {
				problem = `“${file.name}” was rejected: ${asset.rejected_reason ?? 'not a usable image'}`;
			}
		} catch (error) {
			problem = error instanceof ApiError ? `Couldn't upload “${file.name}”: ${error.message}` : `Couldn't upload “${file.name}”.`;
		} finally {
			uploading = '';
			if (fileInput) fileInput.value = '';
		}
	}

	function set(next: string[]) {
		value = field.many ? next : (next[0] ?? '');
		onchange();
	}
	function choose(id: string) {
		if (chosen.includes(id)) return;
		set(field.many ? [...chosen, id] : [id]);
		if (!field.many) open = false;
	}
	function move(index: number, by: number) {
		const next = [...chosen];
		const [item] = next.splice(index, 1);
		if (item !== undefined) next.splice(index + by, 0, item);
		set(next);
	}
</script>

<div class="grid gap-2" data-testid="asset-{field.id}">
	{#if named.length > 0}
		<ol class="grid gap-1.5" aria-label="{field.label}: chosen">
			{#each named as item, index (item.id)}
				<li
					class="flex min-w-0 items-center gap-2 rounded-lg border bg-card px-3 py-1.5 text-sm {refusedAt === index ? 'border-destructive' : 'border-border'}"
					data-testid="asset-chip"
					data-invalid={refusedAt === index || undefined}
				>
					{#if item.asset?.state === 'usable'}
						<img src={assetUrl(project, item.id)} alt="" class="size-8 shrink-0 rounded object-cover" />
					{:else}
						<ImageIcon class="size-4 shrink-0 text-muted-foreground" aria-hidden="true" />
					{/if}
					<span class="min-w-0 flex-1 truncate" data-user-content>{label(item.asset, item.failed)}</span>
					{#if item.asset && item.asset.state !== 'usable'}
						<span class="text-xs text-muted-foreground">{item.asset.state === 'rejected' ? 'Rejected' : 'Checking…'}</span>
					{/if}
					{#if refusedAt === index}<span class="text-xs text-destructive" title={refusal}>Not found</span>{/if}
					{#if field.many && !disabled}
						<Button variant="ghost" size="icon-xs" aria-label="Move {label(item.asset, item.failed)} up" disabled={index === 0} onclick={() => move(index, -1)}><ArrowUpIcon /></Button>
						<Button variant="ghost" size="icon-xs" aria-label="Move {label(item.asset, item.failed)} down" disabled={index === named.length - 1} onclick={() => move(index, 1)}><ArrowDownIcon /></Button>
					{/if}
					{#if !disabled}
						<Button variant="ghost" size="icon-xs" aria-label="Remove {label(item.asset, item.failed)}" onclick={() => set(chosen.filter((id) => id !== item.id))}><XIcon /></Button>
					{/if}
				</li>
			{/each}
		</ol>
	{/if}

	{#if !disabled}
		{#if full}
			<p class="text-xs text-muted-foreground">
				{#if field.many}At most {field.max} {field.max === 1 ? 'image' : 'images'}: remove one to add another.{:else}Remove it to choose another.{/if}
			</p>
		{:else}
			<div class="grid max-w-xl gap-1.5">
				<Button
					variant="outline"
					class="w-fit"
					id={input}
					aria-describedby={describedBy}
					aria-expanded={open}
					onclick={() => (open = !open)}
					data-testid="asset-toggle"
				>
					{chosen.length === 0 ? 'Choose an image' : field.many ? 'Add an image' : 'Change the image'}
				</Button>
				{#if open}
					<div class="rounded-lg border border-border bg-card p-3" data-testid="asset-library">
						<div class="mb-3 flex items-center gap-2">
							<Button variant="outline" size="sm" disabled={uploading !== ''} onclick={() => fileInput?.click()} data-testid="asset-upload">
								{uploading === '' ? 'Upload an image' : `Checking ${uploading}…`}
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
							{#if problem}<p class="text-xs text-destructive" data-testid="asset-problem">{problem}</p>{/if}
						</div>
						{#if library.isPending}
							<p class="text-xs text-muted-foreground">Loading the library…</p>
						{:else if library.isError}
							<p class="text-xs text-destructive">Couldn't list the uploads just now. Try again.</p>
						{:else if visible.length === 0}
							<p class="text-xs text-muted-foreground">No uploads yet — upload an image to choose it.</p>
						{:else}
							<ul class="grid grid-cols-2 gap-2 sm:grid-cols-3" aria-label="Uploads">
								{#each visible as asset (asset.id)}
									<li>
										<button
											type="button"
											class="w-full overflow-hidden rounded-lg border text-left outline-none hover:border-primary focus-visible:border-primary disabled:opacity-60 {chosen.includes(asset.id) ? 'border-primary' : 'border-border'}"
											disabled={chosen.includes(asset.id)}
											onclick={() => choose(asset.id)}
											data-testid="asset-option"
											data-state={asset.state}
										>
											{#if asset.state === 'usable'}
												<img src={assetUrl(project, asset.id)} alt="" class="aspect-video w-full bg-muted object-cover" />
											{:else}
												<div class="flex aspect-video w-full items-center justify-center bg-muted text-xs text-muted-foreground">
													{asset.state === 'rejected' ? 'Rejected' : 'Checking…'}
												</div>
											{/if}
											<span class="block truncate px-2 py-1.5 text-xs" data-user-content>{asset.file_name}</span>
										</button>
									</li>
								{/each}
							</ul>
						{/if}
					</div>
				{/if}
			</div>
		{/if}
	{/if}
</div>
