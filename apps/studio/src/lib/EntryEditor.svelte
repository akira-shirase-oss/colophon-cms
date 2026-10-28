<!--
  The entry editor (steps 1.3, 1.5 and 3.5): every field of the entry's content type as a row,
  rich text with its toolbar, autosave with a status anyone can read, and, when someone else
  saved first, a choice instead of an overwrite. History compares any revision with this draft.
-->
<script lang="ts">
	import { onMount } from 'svelte';
	import { asset } from '$app/paths';
	import type { Editor } from '@tiptap/core';
	import { createQueries, createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { resolve } from '$app/paths';
	import LinkIcon from '@lucide/svelte/icons/link';
	import { openEditor, type RichTextValue, toStored } from '@colophon/editor';
	import RichText from '@colophon/content-renderer/RichText.svelte';
	import CloudAlertIcon from '@lucide/svelte/icons/cloud-alert';
	import CloudCheckIcon from '@lucide/svelte/icons/cloud-check';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import {
		type Content,
		type ContentType,
		type Draft,
		type FieldDefinition,
		getRevision,
		listRevisions,
		type Loaded,
		type Referrer,
		signInUrl,
		type Summary
	} from './api.ts';
	import { linkedQuery } from './references.ts';
	import { shownTitle } from './words.ts';
	import { compare } from './diff.ts';
	import { prettyJson } from './json.ts';
	import { plainError } from './errors.ts';
	import DiffView from './DiffView.svelte';
	import PublishEntry from './PublishEntry.svelte';
	import { ago, meQuery, projectQuery, releasesQuery, typesQuery } from './queries.ts';
	import RichTextToolbar from './RichTextToolbar.svelte';
	import StatusBadge from './shell/StatusBadge.svelte';
	import TechnicalDetails from './shell/TechnicalDetails.svelte';
	import { releaseName } from './words.ts';
	import { blockingProblem, EMPTY_DOCUMENT, type FormValue, formValue, storedValue } from './content.ts';
	import { kept } from './drafts.ts';
	import FieldInputs from './FieldInputs.svelte';
	import { Saver } from './saver.svelte.ts';

	let {
		project,
		collection,
		entry,
		type,
		initial,
		published = null,
		referencedBy = []
	}: {
		project: string;
		collection: string;
		entry: string;
		type: ContentType;
		initial: Loaded;
		/** What readers get now, from the collection's list; null when not published. */
		published?: Summary['published'];
		/** Entries that link to this one, as its read says (D-065). */
		referencedBy?: Referrer[];
	} = $props();

	// "Linked from": each linking entry named by reading it; one the role can't read is said so.
	const allTypes = createQuery(() => typesQuery(project));
	const linking = createQueries(() => ({ queries: referencedBy.map((link) => linkedQuery(project, link.collection, link.entry_id)) }));
	const linkedFrom = $derived(
		referencedBy.map((link, index) => {
			const definition = allTypes.data?.find((item) => item.slug === link.collection)?.definition;
			return {
				link,
				read: linking[index]?.data,
				collectionName: definition?.name ?? link.collection,
				fieldLabel: definition?.fields.find((field) => field.id === link.field)?.label ?? link.field
			};
		})
	);
	/** Published entries that link here: taking this down is refused until they change. */
	const linkedLive = $derived(
		linkedFrom.filter((item) => item.link.published).map((item) => (item.read?.seen ? `“${shownTitle(item.read.title)}”` : 'an entry you can’t see'))
	);

	const queryClient = useQueryClient();
	// Until media uploads arrive (P4), every image previews as the same placeholder.
	const assetUrl = (assetId: string) => `${asset('placeholder.svg')}#${assetId}`;
	// The component is keyed on the entry, so the props it starts with are the ones it keeps.
	// svelte-ignore state_referenced_locally
	const definition = type.definition;
	// What the person's role hides or locks (D-058, step R.3): hidden fields are never shown or
	// sent, so a save can't erase them; locked ones are shown, disabled.
	// svelte-ignore state_referenced_locally
	const hidden = new Set(initial.draft.hidden_fields ?? []);
	// svelte-ignore state_referenced_locally
	const locked = initial.draft.locked_fields ?? [];
	const visible = definition.fields.filter((field) => !hidden.has(field.id));
	const plain = visible.filter((field) => field.type !== 'rich_text');
	const rich = visible.filter((field) => field.type === 'rich_text');
	// svelte-ignore state_referenced_locally
	let latest = $state.raw<Draft>(initial.draft);
	// svelte-ignore state_referenced_locally
	const saver = new Saver(project, collection, entry, definition, initial, (loaded) => {
		latest = loaded.draft;
		queryClient.setQueryData(['draft', project, collection, entry], loaded);
		void queryClient.invalidateQueries({ queryKey: ['entries', project, collection] });
		void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	});
	// What the role may do here, and the newest release this page has seen: publishing goes on top
	// of that one (D-045).
	const mine = createQuery(() => projectQuery(project));
	const who = createQuery(meQuery);
	const releases = createQuery(() => releasesQuery(project));
	const revisions = createQuery(() => ({
		queryKey: ['history', project, collection, entry],
		queryFn: () => listRevisions(project, collection, entry)
	}));

	const hosts: Record<string, HTMLDivElement> = {};
	let editors = $state.raw<Record<string, Editor>>({});
	let form = $state<Record<string, FormValue>>({});
	let unopenable = $state('');

	function current(): Content {
		const content: Content = {};
		for (const field of visible) {
			const editor = editors[field.id];
			const value = field.type === 'rich_text' ? editor && toStored(editor.getJSON()) : storedValue(field, form[field.id]);
			if (value !== undefined) content[field.id] = value;
		}
		return content;
	}

	function changed() {
		// JSON that doesn't read yet can't be sent (it would be lost or stored as text): hold.
		const blocked = blockingProblem(plain, form);
		if (blocked !== '') saver.hold(blocked);
		else saver.edit(current());
	}

	/** Shows `content`, refusing any rich text the editor can't give back exactly (D-022). */
	function show(content: Content) {
		const next: Record<string, Editor> = {};
		for (const field of rich) {
			try {
				next[field.id] = openEditor((content[field.id] as RichTextValue | undefined) ?? EMPTY_DOCUMENT, {
					editorProps: {
						attributes: { role: 'textbox', 'aria-multiline': 'true', 'aria-label': field.label, class: 'prose-colophon min-h-64' }
					},
					editable: !locked.includes(field.id),
					onUpdate: changed
				});
			} catch (error) {
				Object.values(next).forEach((editor) => editor.destroy());
				unopenable = `${field.label}: ${(error as Error).message}`;
				return;
			}
		}
		Object.values(editors).forEach((editor) => editor.destroy());
		for (const field of rich) {
			const host = hosts[field.id];
			const editor = next[field.id];
			if (host && editor) editor.mount(host);
		}
		editors = next;
		form = Object.fromEntries(plain.map((field) => [field.id, formValue(field, content[field.id])]));
		unopenable = '';
	}

	function keepMine() {
		// After a reload the editor shows the server's version; "mine" is what this device kept.
		if (saver.conflict?.restored) show(saver.conflict.mine);
		saver.keepMine();
	}

	function useTheirs() {
		show(saver.useTheirs());
		void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	}

	// Someone else saved: the history this page loaded is out of date.
	$effect(() => {
		if (saver.conflict !== null) void queryClient.invalidateQueries({ queryKey: ['history', project, collection, entry] });
	});

	function shown(field: FieldDefinition, value: unknown): string {
		if (value === undefined) return '(empty)';
		if (field.type === 'boolean') return value === true ? 'Yes' : 'No';
		if (field.type === 'datetime' && typeof value === 'string') return new Date(value).toLocaleString();
		if (field.type === 'json') return prettyJson(value);
		return String(value);
	}

	const standing = $derived(
		published === null ? 'draft' : published.revision_id === latest.revision.id && saver.status === 'saved' ? 'live' : 'changed'
	);
	let tab = $state<'content' | 'history'>('content');

	// Restoring a version (spec's History screen): its fields become the draft and are saved as a new
	// version on top of the current one, so nothing in the history is lost and the restore itself can
	// be undone by restoring again. Fields the role may not change keep their current values; hidden
	// ones are never sent, so the server keeps them.
	const restores = $derived(locked.length < visible.length);
	let restoring = $state<string | null>(null);
	let restoreNote = $state<{ kind: 'done' | 'failed'; text: string } | null>(null);
	async function restore(revision: { id: string; created_at: string }) {
		restoring = revision.id;
		restoreNote = null;
		try {
			const old = await queryClient.fetchQuery({
				queryKey: ['revision', project, collection, entry, revision.id],
				queryFn: () => getRevision(project, collection, entry, revision.id),
				staleTime: Infinity
			});
			const now = current();
			const next: Content = {};
			for (const field of visible) {
				const value = locked.includes(field.id) ? now[field.id] : old.content[field.id];
				if (value !== undefined) next[field.id] = value;
			}
			show(next);
			saver.edit(current());
			saver.saveNow();
			comparing = null;
			tab = 'content';
			restoreNote = { kind: 'done', text: `Restored the version saved ${ago(revision.created_at)}, as a new version. The newer ones stay in the history.` };
		} catch (failure) {
			restoreNote = { kind: 'failed', text: `Not restored. ${plainError(failure, 'change this entry')}` };
		} finally {
			restoring = null;
		}
	}
	// The revision being compared with this draft, and what it held.
	let comparing = $state<string | null>(null);
	const compared = createQuery(() => ({
		queryKey: ['revision', project, collection, entry, comparing],
		queryFn: () => getRevision(project, collection, entry, comparing ?? ''),
		enabled: comparing !== null,
		staleTime: Infinity
	}));

	const label = $derived.by(() => {
		switch (saver.status) {
			case 'saved':
				return 'Saved';
			case 'unsaved':
				return saver.keptOnDevice ? 'Unsaved changes, kept on this device' : 'Unsaved changes';
			case 'saving':
				return 'Saving…';
			case 'retrying':
				return "Can't reach the server. Your changes are kept on this device; retrying.";
			case 'conflict':
				return 'Not saved: someone else saved first';
			case 'refused':
				return `Not saved: ${saver.message}`;
			case 'signed-out':
				return 'Signed out: changes kept on this device';
			case 'held':
				return `Not saved: ${saver.message}`;
		}
	});

	onMount(() => {
		const local = kept(project, entry);
		show(local === null ? initial.draft.content : saver.restore(local, initial.draft));
		const onKey = (event: KeyboardEvent) => {
			if ((event.metaKey || event.ctrlKey) && event.key === 's') {
				event.preventDefault();
				saver.saveNow();
			}
		};
		// Edits are kept on this device, so leaving only warns when they couldn't be.
		const onLeave = (event: BeforeUnloadEvent) => {
			if (saver.status !== 'saved' && !saver.keptOnDevice) event.preventDefault();
		};
		window.addEventListener('keydown', onKey);
		window.addEventListener('beforeunload', onLeave);
		return () => {
			window.removeEventListener('keydown', onKey);
			window.removeEventListener('beforeunload', onLeave);
			saver.destroy();
			Object.values(editors).forEach((editor) => editor.destroy());
		};
	});
</script>

{#snippet version(content: Content)}
	{#each definition.fields as field (field.id)}
		<div class="grid gap-0.5 py-1.5">
			<span class="text-xs font-medium text-muted-foreground">{field.label}</span>
			{#if field.type === 'rich_text' && content[field.id] !== undefined}
				<div class="prose-colophon text-sm">
					<RichText value={content[field.id] as RichTextValue} {assetUrl}>
						{#snippet fallback(problem)}<p>This can't be shown: {problem}</p>{/snippet}
					</RichText>
				</div>
			{:else}
				<span class="text-sm">{shown(field, content[field.id])}</span>
			{/if}
		</div>
	{/each}
{/snippet}

<div class="grid gap-8 xl:grid-cols-[minmax(0,1fr)_17rem]">
	<div class="min-w-0">
		{#if unopenable}
			<p role="alert" class="mb-4 rounded-lg border border-destructive/30 bg-destructive/5 px-4 py-3 text-sm text-destructive">
				This entry can't be opened in the editor without changing it: {unopenable}
			</p>
		{/if}

		{#if saver.conflict}
			<section class="mb-6 rounded-xl border border-attention/40 bg-attention-soft/60 p-5" aria-labelledby="conflict-heading" data-testid="conflict">
				<h2 id="conflict-heading" class="font-semibold text-attention-ink">{saver.message}</h2>
				<p class="mt-1 text-sm text-attention-ink">
					<strong>Keep my version</strong> saves yours on top of theirs; theirs stays in the history.
					<strong>Use their version</strong> discards your unsaved changes.
				</p>
				<div class="mt-4 grid gap-3 md:grid-cols-2">
					<article class="rounded-lg border border-border bg-card p-4" data-testid="theirs">
						<h3 class="mb-2 text-sm font-semibold">Their version</h3>
						{@render version(saver.conflict.theirs.content)}
					</article>
					<article class="rounded-lg border border-border bg-card p-4" data-testid="mine">
						<h3 class="mb-2 text-sm font-semibold">Your version</h3>
						{@render version(saver.conflict.mine)}
					</article>
				</div>
				<div class="mt-4 flex flex-wrap gap-2">
					<Button onclick={keepMine}>Keep my version</Button>
					<Button variant="outline" onclick={useTheirs}>Use their version</Button>
				</div>
			</section>
		{/if}

		<div role="tablist" aria-label="Entry" class="mb-6 inline-flex rounded-lg bg-muted p-1">
			{#each [['content', 'Content'], ['history', 'History']] as const as [value, name] (value)}
				<button
					role="tab"
					id="tab-{value}"
					aria-selected={tab === value}
					aria-controls="panel-{value}"
					tabindex={tab === value ? 0 : -1}
					class="rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 aria-selected:bg-background aria-selected:text-foreground aria-selected:shadow-xs"
					onclick={() => (tab = value)}
					onkeydown={(event) => {
						if (event.key === 'ArrowRight' || event.key === 'ArrowLeft') {
							tab = tab === 'content' ? 'history' : 'content';
							document.getElementById(`tab-${tab}`)?.focus();
						}
					}}
				>
					{name}{#if value === 'history' && revisions.data}<span class="ml-1.5 text-xs text-muted-foreground tabular">{revisions.data.length}</span>{/if}
				</button>
			{/each}
		</div>

		{#if restoreNote}
			<p
				role={restoreNote.kind === 'failed' ? 'alert' : 'status'}
				class="mb-4 rounded-lg px-4 py-3 text-sm {restoreNote.kind === 'failed' ? 'bg-attention-soft text-attention-ink' : 'bg-live-soft text-live-ink'}"
				data-testid="restore-note"
			>
				{restoreNote.text}
			</p>
		{/if}

		<div role="tabpanel" id="panel-content" aria-labelledby="tab-content" hidden={tab !== 'content'}>
			{#if hidden.size > 0 || locked.length > 0}
				<p class="mb-4 rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="role-limits">
					{#if hidden.size > 0}Your role doesn’t show {[...hidden].join(', ')}; saving keeps {hidden.size === 1 ? 'it' : 'them'} as {hidden.size === 1 ? 'it is' : 'they are'}.{/if}
					{#if locked.length > 0}Your role can read but not change {locked.join(', ')}.{/if}
				</p>
			{/if}
			<FieldInputs
				fields={plain}
				bind:form
				titleField={definition.title_field}
				onchange={changed}
				{locked}
				refused={saver.status === 'refused' ? { field: saver.field, place: saver.place, message: saver.message } : null}
				{project}
			/>
			{#each rich as field (field.id)}
				<div class="py-5">
					<p class="mb-2 text-sm font-medium">{field.label}{field.required ? '' : ' (optional)'}</p>
					<div class="overflow-hidden rounded-xl border border-border bg-card shadow-xs focus-within:border-ring/60">
						<RichTextToolbar editor={editors[field.id] ?? null} label="Formatting: {field.label}" />
						<div class="editor px-6 py-5 sm:px-8" bind:this={hosts[field.id]}></div>
					</div>
				</div>
			{/each}
		</div>

		{#if linkedFrom.length > 0}
			<section class="mt-8" aria-labelledby="linked-from-heading" data-testid="linked-from">
				<h2 id="linked-from-heading" class="mb-1 text-sm font-semibold">Linked from</h2>
				<p class="mb-3 text-xs text-muted-foreground">Entries whose fields link to this one.</p>
				<ul class="overflow-hidden rounded-xl border border-border bg-card">
					{#each linkedFrom as item (`${item.link.entry_id}-${item.link.field}`)}
						<li class="rule-dotted flex flex-wrap items-center gap-x-3 gap-y-1 px-4 py-2.5 text-sm last:border-b-0" data-testid="linked-from-entry">
							<LinkIcon class="size-3.5 shrink-0 text-muted-foreground" aria-hidden="true" />
							<span class="min-w-0 flex-1">
								{#if item.read === undefined}
									<span class="text-muted-foreground">Loading…</span>
								{:else if item.read.seen}
									<a
										class="font-medium hover:underline"
										data-user-content
										href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: item.link.collection, entry: item.link.entry_id })}
										>{shownTitle(item.read.title)}</a
									>
								{:else}
									<span class="text-muted-foreground">An entry you can’t see</span>
								{/if}
								<span class="block text-xs text-muted-foreground" data-user-content>{item.collectionName}, in {item.fieldLabel}</span>
							</span>
							{#if item.link.published}
								<Badge variant="live">Readers see this link</Badge>
							{:else}
								<Badge variant="draft">In a draft</Badge>
							{/if}
						</li>
					{/each}
				</ul>
			</section>
		{/if}

		<div role="tabpanel" id="panel-history" aria-labelledby="tab-history" hidden={tab !== 'history'}>
			{#if revisions.data}
				<ol data-testid="history" class="relative space-y-1 border-l border-dotted border-border pl-6">
					{#each revisions.data as revision, index (revision.id)}
						<li class="relative">
							<span class="absolute top-4 -left-[1.6rem] size-2.5 rounded-full border-2 border-background {index === 0 ? 'bg-foreground' : 'bg-border'}" aria-hidden="true"></span>
							<div class="flex flex-wrap items-center gap-x-3 gap-y-1 rounded-lg px-3 py-2.5 hover:bg-muted/50">
								<span class="min-w-0 flex-1">
									<span class="block truncate text-sm font-medium">{revision.title}</span>
									<span class="text-xs text-muted-foreground" title={new Date(revision.created_at).toLocaleString()}>Saved {ago(revision.created_at)}</span>
								</span>
								{#if index === 0}<Badge variant="secondary">Current draft</Badge>{/if}
								{#if published?.revision_id === revision.id}<Badge variant="live">Live in {releaseName(published.generation)}</Badge>{/if}
								{#if index > 0}
									<Button variant="ghost" size="xs" onclick={() => (comparing = comparing === revision.id ? null : revision.id)}>
										{comparing === revision.id ? 'Hide' : 'Compare with draft'}
									</Button>
									{#if restores}
										<Button
											variant="outline"
											size="xs"
											disabled={restoring !== null || saver.status !== 'saved'}
											title={saver.status !== 'saved' ? 'Wait until your changes are saved' : undefined}
											onclick={() => restore(revision)}
										>
											{restoring === revision.id ? 'Restoring…' : 'Restore this version'}
										</Button>
									{/if}
								{/if}
							</div>
							{#if comparing === revision.id}
								<div class="unfold mx-3 mb-3 rounded-lg border border-border bg-card p-4">
									{#if compared.data}
										<DiffView changes={compare(definition, compared.data.content, current())} before="this version" after="the draft" />
									{:else if compared.isError}
										<p class="text-sm text-destructive">{compared.error.message}</p>
									{:else}
										<p class="text-sm text-muted-foreground">Loading…</p>
									{/if}
								</div>
							{/if}
						</li>
					{/each}
				</ol>
			{/if}
		</div>
	</div>

	<!--
	  Saving and publishing: a card beside the fields on wide screens; below xl, a bar that stays at
	  the bottom of the screen while the fields scroll, so Publish never sits under a long body
	  (D-059, rule 7). The details and Technical details are on the wide card only.
	-->
	<aside
		class="sticky bottom-0 z-20 -mx-5 border-t border-border bg-background/95 px-5 py-3 backdrop-blur sm:-mx-8 sm:px-8 lg:-mx-10 lg:px-10 xl:top-20 xl:bottom-auto xl:mx-0 xl:self-start xl:border-t-0 xl:bg-transparent xl:p-0 xl:backdrop-blur-none"
		aria-label="Saving and publishing"
		data-testid="entry-actions"
	>
		<div class="grid gap-x-6 gap-y-2 sm:grid-cols-[minmax(0,1fr)_minmax(0,18rem)] sm:items-center xl:block xl:rounded-xl xl:border xl:border-border xl:bg-card xl:p-4 xl:shadow-xs">
			<div class="min-w-0">
			<p class="flex items-start gap-2 text-sm font-medium" role="status" data-testid="save-status" data-status={saver.status}>
				{#if saver.status === 'saved'}
					<CloudCheckIcon class="pop mt-0.5 size-4 shrink-0 text-live" aria-hidden="true" />
				{:else if saver.status === 'saving'}
					<LoaderIcon class="mt-0.5 size-4 shrink-0 animate-spin text-muted-foreground" aria-hidden="true" />
				{:else if saver.status === 'unsaved'}
					<span class="breathe mt-1.5 size-2 shrink-0 rounded-full bg-pending" aria-hidden="true"></span>
				{:else}
					<CloudAlertIcon class="mt-0.5 size-4 shrink-0 text-destructive" aria-hidden="true" />
				{/if}
				<span class:text-destructive={saver.status === 'conflict' || saver.status === 'refused' || saver.status === 'signed-out' || saver.status === 'held'}>{label}</span>
			</p>
			{#if saver.message && saver.status !== 'refused' && saver.status !== 'conflict' && saver.status !== 'held'}
				<p class="mt-2 text-xs text-muted-foreground" data-testid="save-message">{saver.message}</p>
			{/if}
			{#if saver.status === 'signed-out'}
				<p class="mt-2 text-xs"><a class="underline" href={signInUrl(window.location.pathname)} data-sveltekit-reload>Sign in again</a> to save them.</p>
			{/if}
			</div>
			<Button class="mt-3 hidden w-full xl:flex" variant="outline" size="sm" onclick={() => saver.saveNow()} disabled={saver.status === 'saved' || saver.status === 'conflict' || saver.status === 'held'}>
				Save now <kbd class="ml-auto font-sans text-[0.6875rem] text-muted-foreground">⌘S</kbd>
			</Button>

			<dl class="mt-4 hidden space-y-2.5 border-t border-dotted border-border pt-4 text-sm xl:block">
				<div class="flex items-center justify-between gap-2">
					<dt class="text-muted-foreground">Status</dt>
					<dd><StatusBadge state={standing} generation={published?.generation ?? null} /></dd>
				</div>
				<div class="flex items-center justify-between gap-2">
					<dt class="text-muted-foreground">Live in</dt>
					<dd class="tabular">{published ? releaseName(published.generation) : '—'}</dd>
				</div>
				<div class="flex items-center justify-between gap-2">
					<dt class="text-muted-foreground">Saved</dt>
					<dd title={new Date(latest.updated_at).toLocaleString()}>{ago(latest.updated_at)}</dd>
				</div>
				<div class="flex items-center justify-between gap-2">
					<dt class="text-muted-foreground">Created</dt>
					<dd>{new Date(latest.created_at).toLocaleDateString()}</dd>
				</div>
			</dl>
			<div class="xl:mt-4 xl:border-t xl:border-dotted xl:border-border xl:pt-4">
				<PublishEntry
					{project}
					{collection}
					{entry}
					title={latest.revision.title}
					mine={mine.data}
					{standing}
					saved={saver.status === 'saved'}
					generation={published?.generation ?? null}
					seenGeneration={releases.data?.generation}
					own={who.data != null && latest.created_by === who.data.id}
					{linkedLive}
				/>
			</div>
			<TechnicalDetails class="mt-4 hidden border-t border-dotted border-border pt-3 xl:block">
				<p>Entry <span class="font-mono break-all">{entry}</span> in collection <span class="font-mono">{collection}</span></p>
				<p>Revision <span class="font-mono break-all">{latest.revision.id}</span></p>
				<p>Content hash <span class="font-mono break-all">{latest.revision.content_hash}</span></p>
				<p>Readers get an entry only through a changeset that is published as a release.</p>
			</TechnicalDetails>
		</div>
	</aside>
</div>
