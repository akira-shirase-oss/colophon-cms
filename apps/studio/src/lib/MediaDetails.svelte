<!--
  One upload, in full (D2): a large preview, the library's words for it (alt text, title, credit,
  tags) saved under If-Match so two people never overwrite each other silently, the versions its
  bytes have had, where it is used, a replacement (published pages keep the old picture until
  the entries using it are published again, so this says so and offers the publish request),
  its two addresses to copy, and the staged delete. Reads refresh live with the library: an
  `assets` hint invalidates ['assets', project], under which these queries sit.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import ImageIcon from '@lucide/svelte/icons/image';
	import RefreshCwIcon from '@lucide/svelte/icons/refresh-cw';
	import TrashIcon from '@lucide/svelte/icons/trash-2';
	import { resolve } from '$app/paths';
	import { createQueries, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { untrack } from 'svelte';
	import { toast } from 'svelte-sonner';
	import * as Alert from '@colophon/ui/ui/alert';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import * as Sheet from '@colophon/ui/ui/sheet';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import {
		ApiError,
		type Asset,
		assetUrl,
		createChangeset,
		getAssetWithVersions,
		getEntry,
		listAssetReferences,
		newKey,
		replaceAsset,
		updateAsset
	} from './api.ts';
	import { plainError } from './errors.ts';
	import {
		absolute,
		altOf,
		changed,
		currentSha,
		deletable,
		fieldAt,
		formOf,
		formProblems,
		LIMITS,
		measure,
		type MetadataForm,
		metadataOf,
		previewUrl,
		replaceable,
		sentence,
		size,
		stateExplained,
		stateText,
		versionUrl
	} from './library.ts';
	import { ago, typesQuery } from './queries.ts';
	import Problem from './shell/Problem.svelte';
	import TechnicalDetails from './shell/TechnicalDetails.svelte';
	import { PUBLISH_REQUEST, shownTitle } from './words.ts';

	let {
		project,
		asset: row,
		open = $bindable(false),
		edits = true,
		deletes = true,
		onfilter,
		ondelete
	}: {
		project: string;
		/** The upload as the grid last read it: shown at once while the full record loads. */
		asset: Asset | null;
		open?: boolean;
		/** Whether the role may change uploads (metadata, replacement). */
		edits?: boolean;
		/** Whether the role may delete uploads. */
		deletes?: boolean;
		/** Narrows the library to one tag. */
		onfilter: (tag: string) => void;
		/** Asks to delete it (the page confirms). */
		ondelete: (asset: Asset) => void;
	} = $props();

	const id = $derived(row?.id ?? '');
	const queryClient = useQueryClient();
	// Replacement: new bytes for the same upload. The current version keeps serving until the new
	// one is checked, and published pages keep the version their release recorded. While one is
	// being checked the record is read every two seconds until its new version shows.
	let replaceInput = $state<HTMLInputElement>();
	let replacing = $state<'idle' | 'sending' | 'queued' | 'error'>('idle');
	let replaceFailure = $state('');
	let versionsBefore = $state(0);
	const details = createQuery(() => ({
		queryKey: ['assets', project, 'details', id],
		queryFn: () => getAssetWithVersions(project, id),
		enabled: open && id !== '',
		staleTime: 2_000,
		refetchInterval: (query: { state: { data?: { versions: unknown[] } } }) =>
			replacing === 'queued' && (query.state.data?.versions.length ?? 0) <= versionsBefore ? 2_000 : false
	}));
	const references = createQuery(() => ({
		queryKey: ['assets', project, 'references', id],
		queryFn: () => listAssetReferences(project, id),
		enabled: open && id !== '',
		staleTime: 2_000
	}));
	const types = createQuery(() => ({ ...typesQuery(project), enabled: open }));
	const current = $derived(details.data?.id === id ? details.data : row);

	// Each entry that uses it, once (a list field or two fields may name it twice), at most ten read
	// for their titles.
	const users = $derived.by(() => {
		const seen = new Map<string, { entry: string; collection: string; draft: boolean; published: boolean; fields: string[] }>();
		for (const reference of references.data ?? []) {
			const known = seen.get(reference.entry_id);
			if (known === undefined) {
				seen.set(reference.entry_id, {
					entry: reference.entry_id,
					collection: reference.collection,
					draft: reference.in_draft,
					published: reference.published,
					fields: [reference.field]
				});
			} else {
				known.draft ||= reference.in_draft;
				known.published ||= reference.published;
				if (!known.fields.includes(reference.field)) known.fields.push(reference.field);
			}
		}
		return [...seen.values()];
	});
	const SHOWN_USERS = 10;
	const titles = createQueries(() => ({
		queries: users.slice(0, SHOWN_USERS).map((user) => ({
			queryKey: ['assets', project, 'user', user.collection, user.entry],
			queryFn: async () => (await getEntry(project, user.collection, user.entry)).draft.revision.title,
			staleTime: 30_000,
			retry: false
		}))
	}));
	const typeName = (slug: string) => types.data?.find((type) => type.slug === slug)?.definition.name ?? slug;
	const published = $derived(users.filter((user) => user.published));

	// The metadata form: loaded from the record, kept while someone is typing even if the record
	// refreshes underneath (a save then meets the newer version and says so).
	let form = $state<MetadataForm>({ alt: '', title: '', credit: '', tags: '' });
	let baseline = $state<Asset | null>(null);
	let status = $state<'idle' | 'saving' | 'saved' | 'conflict' | 'error'>('idle');
	let failure = $state('');
	let refused = $state<Partial<Record<keyof MetadataForm, string>>>({});
	let attempt: { body: string; version: number; key: string } | null = null;
	const dirty = $derived(baseline !== null && changed(form, baseline));
	const problems = $derived({ ...formProblems(form), ...refused });
	$effect(() => {
		const latest = current;
		if (latest === null || latest === undefined) return;
		untrack(() => {
			const fresh = baseline === null || baseline.id !== latest.id;
			if (fresh || (!dirty && baseline !== null && baseline.metadata_version !== latest.metadata_version && status !== 'saving')) {
				form = formOf(latest);
				baseline = latest;
				refused = {};
				if (fresh) status = 'idle';
			}
		});
	});
	// Typing after a save or a refusal starts over.
	$effect(() => {
		void form.alt;
		void form.title;
		void form.credit;
		void form.tags;
		untrack(() => {
			refused = {};
			if (status === 'saved' || status === 'error') status = 'idle';
		});
	});

	async function save(event?: SubmitEvent) {
		event?.preventDefault();
		const base = baseline;
		if (base === null || status === 'saving' || Object.keys(formProblems(form)).length > 0) return;
		const body = metadataOf(form);
		const text = JSON.stringify(body);
		// The same edit sent again (after no answer) keeps its key, so it can't apply twice.
		if (attempt === null || attempt.body !== text || attempt.version !== base.metadata_version) {
			attempt = { body: text, version: base.metadata_version, key: newKey() };
		}
		status = 'saving';
		failure = '';
		try {
			const updated = await updateAsset(project, base.id, body, base.metadata_version, attempt.key);
			attempt = null;
			queryClient.setQueryData(['assets', project, 'details', base.id], (old: typeof details.data) => (old === undefined ? old : { ...old, ...updated }));
			void queryClient.invalidateQueries({ queryKey: ['assets', project] });
			baseline = updated;
			form = formOf(updated);
			// Set after the form, so the typing effect above doesn't clear it.
			queueMicrotask(() => (status = 'saved'));
		} catch (error) {
			if (error instanceof ApiError && error.failure === 'conflict') {
				status = 'conflict';
			} else if (error instanceof ApiError && error.status === 422 && fieldAt(error.details?.pointer) !== null) {
				const field = fieldAt(error.details?.pointer);
				if (field !== null) refused = { [field]: plainError(error, 'save these details') };
				status = 'idle';
			} else {
				status = 'error';
				failure = plainError(error, 'save these details');
			}
		}
	}

	/** Conflict: drop what was typed and show what is stored now. */
	async function theirs() {
		const fresh = (await details.refetch()).data;
		if (fresh === undefined) return;
		form = formOf(fresh);
		baseline = fresh;
		queueMicrotask(() => (status = 'idle'));
	}

	/** Conflict: save what this person changed on top of the newer version — their other changes
	 *  stay, the fields typed here win. */
	async function mine() {
		const before = baseline;
		const fresh = (await details.refetch()).data;
		if (fresh === undefined || before === null) return;
		const ours = metadataOf(formOf(before));
		const typed = metadataOf(form);
		const merged = formOf(fresh);
		for (const field of ['alt', 'title', 'credit', 'tags'] as const) {
			if (JSON.stringify(typed[field]) !== JSON.stringify(ours[field])) merged[field] = form[field];
		}
		form = merged;
		baseline = fresh;
		status = 'idle';
		await save();
	}

	async function replace(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const file = input.files?.[0];
		const target = current;
		if (file === undefined || target === null || target === undefined) return;
		replacing = 'sending';
			replaceFailure = '';
		try {
			versionsBefore = details.data?.versions.length ?? 0;
			await replaceAsset(project, target.id, file);
			replacing = 'queued';
			void queryClient.invalidateQueries({ queryKey: ['assets', project] });
		} catch (error) {
			replacing = 'error';
			replaceFailure = plainError(error, 'replace this file');
		} finally {
			input.value = '';
		}
	}

	// The publish request for the entries that show the old picture.
	let asking = $state(false);
	let asked = $state<{ id: string; count: number } | null>(null);
	let askFailure = $state('');
	async function askToPublish() {
		const target = current;
		if (target === null || target === undefined || published.length === 0 || asking) return;
		asking = true;
		askFailure = '';
		try {
			const title = `Show the new “${target.file_name.slice(0, 80)}”`;
			const made = await createChangeset(
				project,
				title,
				published.map((user) => user.entry),
				[],
				newKey()
			);
			asked = { id: made.id, count: published.length };
			void queryClient.invalidateQueries({ queryKey: ['changesets', project] });
		} catch (error) {
			askFailure = plainError(error, `make a ${PUBLISH_REQUEST}`);
		} finally {
			asking = false;
		}
	}

	async function copy(path: string, what: string) {
		try {
			await navigator.clipboard.writeText(absolute(path, window.location.origin));
			toast.success(`${what} copied`);
		} catch {
			toast.error("Couldn't copy. Open Technical details below and copy the address from there.");
		}
	}

	// A different upload opened: its own replacement and request state.
	$effect(() => {
		void id;
		untrack(() => {
			replacing = 'idle';
			replaceFailure = '';
			asked = null;
			askFailure = '';
		});
	});

	const preview = $derived(current ? previewUrl(project, current) : null);
	const versions = $derived([...(details.data?.versions ?? [])].reverse());
	const fieldId = $props.id();
	const counter = (value: string, limit: number) => `${[...value.trim()].length} of ${limit}`;
</script>

<Sheet.Root bind:open>
	<Sheet.Content class="gap-0 p-0 data-[side=right]:w-full data-[side=right]:sm:max-w-xl" data-testid="media-details">
		{#if current}
			<Sheet.Header class="border-b border-border px-5 pt-5 pb-4 pr-14">
				<Sheet.Title class="truncate" data-user-content>{current.file_name}</Sheet.Title>
				<Sheet.Description class="flex flex-wrap items-center gap-x-2 gap-y-1">
					<Badge
						variant={current.state === 'usable' ? 'live' : current.state === 'rejected' ? 'attention' : current.state === 'quarantined' ? 'pending' : 'secondary'}
						data-testid="details-state">{stateText(current.state)}</Badge
					>
					<span>
						{measure(current)} · added {ago(current.created_at)}
					</span>
				</Sheet.Description>
			</Sheet.Header>

			<div class="grid gap-7 overflow-y-auto px-5 py-5" data-testid="details-body">
				<figure class="grid gap-2">
					<div class="flex max-h-[50vh] min-h-40 items-center justify-center overflow-hidden rounded-xl border border-border bg-secondary/40">
						{#if preview !== null}
							<img
								src={preview}
								alt={altOf(current)}
								width={current.width ?? undefined}
								height={current.height ?? undefined}
								class="h-auto max-h-[50vh] w-auto max-w-full object-contain"
								data-testid="details-preview"
							/>
						{:else}
							<div class="flex flex-col items-center gap-2 px-6 py-10 text-center">
								<ImageIcon class="size-8 text-muted-foreground" aria-hidden="true" />
							</div>
						{/if}
					</div>
					<figcaption class="text-sm text-muted-foreground">
						{stateExplained(current.state)}
						{#if current.rejected_reason}<span class="text-attention-ink" data-testid="details-reason"> {sentence(current.rejected_reason)}</span>{/if}
					</figcaption>
				</figure>

				<section aria-labelledby="{fieldId}-details" class="grid gap-4">
					<h3 id="{fieldId}-details" class="font-medium">Description</h3>
					{#if edits}
						<form class="grid gap-4" onsubmit={save} data-testid="details-form">
							<div class="grid gap-1.5">
								<div class="flex items-baseline justify-between gap-2">
									<Label for="{fieldId}-alt">Alt text</Label>
									<span class="text-xs text-muted-foreground tabular-nums">{counter(form.alt, LIMITS.alt)}</span>
								</div>
								<Textarea
									id="{fieldId}-alt"
									bind:value={form.alt}
									rows={2}
									aria-invalid={problems.alt !== undefined}
									aria-describedby="{fieldId}-alt-help"
									data-testid="details-alt"
								/>
								<p id="{fieldId}-alt-help" class="text-xs {problems.alt ? 'text-destructive' : 'text-muted-foreground'}">
									{problems.alt ?? "What the picture shows, for people who can't see it. An image put into text starts with this."}
								</p>
							</div>
							<div class="grid gap-4 sm:grid-cols-2">
								<div class="grid gap-1.5">
									<Label for="{fieldId}-title">Title</Label>
									<Input id="{fieldId}-title" bind:value={form.title} aria-invalid={problems.title !== undefined} aria-describedby="{fieldId}-title-help" data-testid="details-title" />
									{#if problems.title}<p id="{fieldId}-title-help" class="text-xs text-destructive">{problems.title}</p>{/if}
								</div>
								<div class="grid gap-1.5">
									<Label for="{fieldId}-credit">Credit</Label>
									<Input
										id="{fieldId}-credit"
										bind:value={form.credit}
										placeholder="Photo: …"
										aria-invalid={problems.credit !== undefined}
										aria-describedby="{fieldId}-credit-help"
										data-testid="details-credit"
									/>
									{#if problems.credit}<p id="{fieldId}-credit-help" class="text-xs text-destructive">{problems.credit}</p>{/if}
								</div>
							</div>
							<div class="grid gap-1.5">
								<Label for="{fieldId}-tags">Tags</Label>
								<Input
									id="{fieldId}-tags"
									bind:value={form.tags}
									placeholder="hero, team, 2026"
									aria-invalid={problems.tags !== undefined}
									aria-describedby="{fieldId}-tags-help"
									autocapitalize="off"
									spellcheck={false}
									data-testid="details-tags"
								/>
								<p id="{fieldId}-tags-help" class="text-xs {problems.tags ? 'text-destructive' : 'text-muted-foreground'}">
									{problems.tags ?? `Up to ${LIMITS.tags}, separated by commas: lowercase letters, digits and hyphens.`}
								</p>
							</div>

							{#if status === 'conflict'}
								<Alert.Root variant="destructive" data-testid="details-conflict">
									<Alert.Title>Someone changed this upload's details while you were editing</Alert.Title>
									<Alert.Description>
										<p>
											Keep mine saves what you changed here on top of their version; their other changes stay. Load their changes drops
											what you typed. Nothing is lost until you choose.
										</p>
										<div class="mt-2 flex flex-wrap gap-2">
											<Button variant="outline" size="sm" onclick={theirs} data-testid="conflict-theirs">Load their changes</Button>
											<Button size="sm" onclick={mine} data-testid="conflict-mine">Keep mine</Button>
										</div>
									</Alert.Description>
								</Alert.Root>
							{:else if status === 'error'}
								<Alert.Root variant="destructive" data-testid="details-error">
									<Alert.Title>Couldn't save</Alert.Title>
									<Alert.Description><p>{failure}</p></Alert.Description>
								</Alert.Root>
							{/if}

							<div class="flex flex-wrap items-center justify-end gap-3">
								<p class="text-sm text-muted-foreground" role="status" data-testid="details-status">
									{#if status === 'saving'}Saving…{:else if status === 'saved'}Saved{:else if dirty}Not saved yet{/if}
								</p>
								<Button type="submit" disabled={!dirty || status === 'saving' || status === 'conflict' || Object.keys(formProblems(form)).length > 0} data-testid="details-save">
									{status === 'error' ? 'Try again' : 'Save'}
								</Button>
							</div>
						</form>
					{:else}
						<dl class="grid gap-3 text-sm">
							<div><dt class="text-muted-foreground">Alt text</dt><dd data-user-content>{current.alt ?? 'None yet'}</dd></div>
							<div><dt class="text-muted-foreground">Title</dt><dd data-user-content>{current.title ?? 'None'}</dd></div>
							<div><dt class="text-muted-foreground">Credit</dt><dd data-user-content>{current.credit ?? 'None'}</dd></div>
						</dl>
					{/if}
					{#if current.tags.length > 0}
						<div class="flex flex-wrap items-center gap-1.5" aria-label="Show uploads with a tag" role="group">
							{#each current.tags as tag (tag)}
								<Button variant="outline" size="sm" class="h-7 rounded-full px-2.5 text-xs" onclick={() => onfilter(tag)} aria-label="Show uploads tagged {tag}"
									>#{tag}</Button
								>
							{/each}
						</div>
					{/if}
				</section>

				<section aria-labelledby="{fieldId}-used" class="grid gap-3">
					<h3 id="{fieldId}-used" class="font-medium">Where it's used</h3>
					{#if references.isPending}
						<Skeleton class="h-12 w-full rounded-lg" />
					{:else if references.isError}
						<Problem error={references.error} what="where it's used" retry={() => references.refetch()} />
					{:else if users.length === 0}
						<p class="text-sm text-muted-foreground" data-testid="details-unused">Nothing uses this yet. It can be deleted.</p>
					{:else}
						<ul class="grid gap-1.5" data-testid="details-users">
							{#each users.slice(0, SHOWN_USERS) as user, index (user.entry)}
								<li class="flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-lg border border-border px-3 py-2 text-sm">
									<a
										class="min-w-0 truncate font-medium underline-offset-4 hover:underline focus-visible:rounded-sm focus-visible:ring-[3px] focus-visible:ring-ring/40 focus-visible:outline-none"
										href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: user.collection, entry: user.entry })}
										data-user-content
										>{titles[index]?.data !== undefined ? shownTitle(titles[index].data) : titles[index]?.isError ? 'An entry' : 'Loading…'}</a
									>
									<span class="flex shrink-0 items-center gap-1.5 text-xs text-muted-foreground">
										{typeName(user.collection)}
										{#if user.published}<Badge variant="live">Published</Badge>{/if}
										{#if user.draft && !user.published}<Badge variant="draft">Draft</Badge>{/if}
									</span>
								</li>
							{/each}
						</ul>
						{#if users.length > SHOWN_USERS}
							<p class="text-xs text-muted-foreground">And {users.length - SHOWN_USERS} more.</p>
						{/if}
					{/if}
				</section>

				{#if edits && replaceable(current)}
					<section aria-labelledby="{fieldId}-replace" class="grid gap-3">
						<h3 id="{fieldId}-replace" class="font-medium">Replace the file</h3>
						<p class="text-sm text-muted-foreground" data-testid="replace-warning">
							Published pages keep the old image until you publish again. The new file is checked first; until it is ready, the current one
							keeps showing everywhere.
						</p>
						<div class="flex flex-wrap items-center gap-2">
							<Button variant="outline" onclick={() => replaceInput?.click()} disabled={replacing === 'sending'} data-testid="replace-asset">
								<RefreshCwIcon data-icon="inline-start" />{replacing === 'sending' ? 'Sending…' : 'Choose a new file'}
							</Button>
							<input bind:this={replaceInput} type="file" accept="image/png,image/jpeg,image/webp" class="sr-only" tabindex="-1" aria-hidden="true" onchange={replace} data-testid="replace-file" />
						</div>
						{#if replacing === 'queued'}
							<p class="text-sm" role="status" data-testid="replace-queued">
								The new file is being checked. It becomes the current version when it's ready; the versions below show when.
							</p>
							{#if published.length > 0}
								<div class="grid gap-2 rounded-lg border border-border bg-muted/40 p-3 text-sm" data-testid="replace-publish">
									<p>
										{published.length === 1 ? '1 published entry still shows' : `${published.length} published entries still show`} the old image. A
										{PUBLISH_REQUEST} for their current drafts goes to review first; any unpublished edits in them go too.
									</p>
									{#if asked}
										<a
											class="font-medium underline underline-offset-4"
											href={resolve('/projects/[project]/changesets/[changeset]', { project, changeset: asked.id })}
											data-testid="replace-request">Open the {PUBLISH_REQUEST}</a
										>
									{:else}
										<div>
											<Button size="sm" onclick={askToPublish} disabled={asking} data-testid="replace-ask">
												{asking ? 'Making it…' : `Ask to publish ${published.length === 1 ? 'it' : `these ${published.length}`}`}
											</Button>
										</div>
										{#if askFailure !== ''}<p class="text-destructive" role="alert">{askFailure}</p>{/if}
									{/if}
								</div>
							{/if}
						{:else if replacing === 'error'}
							<p class="text-sm text-destructive" role="alert" data-testid="replace-error">{replaceFailure}</p>
						{/if}
					</section>
				{/if}

				<section aria-labelledby="{fieldId}-versions" class="grid gap-3">
					<h3 id="{fieldId}-versions" class="font-medium">Versions</h3>
					{#if details.isPending}
						<Skeleton class="h-12 w-full rounded-lg" />
					{:else if details.isError}
						<Problem error={details.error} what="this upload's versions" retry={() => details.refetch()} />
					{:else if versions.length === 0}
						<p class="text-sm text-muted-foreground">No checked version yet.</p>
					{:else}
						<ol class="grid gap-1.5" data-testid="details-versions">
							{#each versions as version (version.seq)}
								<li class="flex min-w-0 flex-wrap items-center justify-between gap-2 rounded-lg border border-border px-3 py-2 text-sm">
									<span class="flex items-center gap-2">
										<span class="font-medium">Version {version.seq}</span>
										{#if version.sha256 === currentSha(current)}<Badge variant="live">Current</Badge>{/if}
									</span>
									<span class="flex items-center gap-2 text-xs text-muted-foreground">
										{version.width}×{version.height} · {size(version.size_bytes)} · {ago(version.created_at)}
										<Button
											variant="ghost"
											size="icon-sm"
											onclick={() => copy(versionUrl(project, current.id, version.sha256), `Version ${version.seq}'s address`)}
											aria-label="Copy version {version.seq}'s address"
											title="Copy this version's address"><CopyIcon /></Button
										>
									</span>
								</li>
							{/each}
						</ol>
					{/if}
				</section>

				{#if current.state === 'usable'}
					<section aria-labelledby="{fieldId}-addresses" class="grid gap-3">
						<h3 id="{fieldId}-addresses" class="font-medium">Use it elsewhere</h3>
						<div class="flex flex-wrap gap-2">
							<Button variant="outline" size="sm" onclick={() => copy(assetUrl(project, current.id), 'Address')} data-testid="copy-current">
								<CopyIcon data-icon="inline-start" />Copy address
							</Button>
							{#if currentSha(current) !== null}
								<Button
									variant="outline"
									size="sm"
									onclick={() => copy(versionUrl(project, current.id, currentSha(current) ?? ''), "This version's address")}
									data-testid="copy-version"
								>
									<CopyIcon data-icon="inline-start" />Copy this version's address
								</Button>
							{/if}
						</div>
						<p class="text-xs text-muted-foreground">
							The address always shows the newest version. This version's address never changes, even after a replacement.
						</p>
					</section>
				{/if}

				<TechnicalDetails>
					<p class="break-all">id {current.id}</p>
					<p>declared {current.declared_type} · detected {current.detected_type ?? 'not yet'}</p>
					{#if currentSha(current) !== null}<p class="break-all">sha256 {currentSha(current)}</p>{/if}
					<p class="break-all">{absolute(assetUrl(project, current.id), window.location.origin)}</p>
					<p>metadata_version {current.metadata_version}</p>
				</TechnicalDetails>

				{#if deletes}
					<div class="flex justify-end border-t border-border pt-4">
						<Button variant="ghost" disabled={!deletable(current)} onclick={() => ondelete(current)} data-testid="details-delete">
							<TrashIcon data-icon="inline-start" />Delete…
						</Button>
					</div>
				{/if}
			</div>
		{:else}
			<Sheet.Header class="px-5 pt-5">
				<Sheet.Title>Upload</Sheet.Title>
				<Sheet.Description>Loading…</Sheet.Description>
			</Sheet.Header>
		{/if}
	</Sheet.Content>
</Sheet.Root>
