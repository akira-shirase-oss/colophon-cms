<!--
  Settings → Review stages (D-104, E2): the named sign-offs a publish request needs, in order.
  With none, one approval publishes — the way it always worked; with a list, every stage needs
  an approval on the version being published, a request for changes at any stage blocks it, and
  a stage marked "someone other than the sender" never counts the sender's own approval. Saving
  replaces the whole list on the etag it was read at (a changed list is refused `stale_stages`,
  like a stale save); removing a stage drops its gate while its recorded reviews stay history.
  Any member may read the list, so it shows read-only to a role that can't change it.
-->
<script lang="ts">
	import ArrowDownIcon from '@lucide/svelte/icons/arrow-down';
	import ArrowUpIcon from '@lucide/svelte/icons/arrow-up';
	import ListChecksIcon from '@lucide/svelte/icons/list-checks';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import XIcon from '@lucide/svelte/icons/x';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, may, newKey, replaceReviewStages, type ReviewStage } from '#lib/api.ts';
	import { slugify } from '#lib/content.ts';
	import { plainError } from '#lib/errors.ts';
	import { projectQuery, stagesQuery } from '#lib/queries.ts';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const stages = createQuery(() => stagesQuery(project));
	const changes = $derived(may(mine.data, 'schema.change'));

	interface Row extends ReviewStage {
		/** The key still follows the name (until the person edits the key itself). */
		auto: boolean;
	}
	let draft = $state<Row[]>([]);
	let loaded = $state(false);
	// Seed once, when the list first reads; an unsaved edit is never clobbered by a refetch.
	$effect(() => {
		if (stages.data !== undefined && !loaded) {
			draft = stages.data.stages.map((stage) => ({ ...stage, auto: false }));
			loaded = true;
		}
	});
	const strip = (rows: Row[]): ReviewStage[] => rows.map(({ key, name, different_reviewer }) => ({ key, name, different_reviewer }));
	const dirty = $derived(loaded && stages.data !== undefined && JSON.stringify(strip(draft)) !== JSON.stringify(stages.data.stages));
	/** The key a name suggests: a role key's shape (lowercase letter, then letters, digits and
	 *  hyphens) — the server checks it again. */
	const keyify = (name: string) => slugify(name).replace(/^[^a-z]+/, '').slice(0, 63);
	const KEY_SHAPE = /^[a-z][a-z0-9-]{0,62}$/;
	const rowProblem = $derived.by(() => {
		if (draft.length > 20) return 'A review workflow has at most 20 stages.';
		for (const [index, row] of draft.entries()) {
			if (row.name.trim() === '') return `Stage ${index + 1} needs a name.`;
			if (!KEY_SHAPE.test(row.key)) return `Stage ${index + 1} needs a short name: a lowercase letter, then letters, digits and hyphens.`;
			if (draft.findIndex((other) => other.key === row.key) !== index) return `Stage ${index + 1} shares its short name with another stage.`;
		}
		return '';
	});

	let error = $state('');
	let stale = $state(false);
	let busy = $state(false);
	// One key per save attempt: pressing Save again after a lost answer is safe.
	let saveKey = newKey();

	function addStage() {
		draft = [...draft, { key: '', name: '', different_reviewer: false, auto: true }];
	}
	function nameAt(index: number, name: string) {
		const row = draft[index];
		if (row === undefined) return;
		row.name = name;
		if (row.auto) row.key = keyify(name);
		draft = draft;
	}
	function keyAt(index: number, key: string) {
		const row = draft[index];
		if (row === undefined) return;
		row.key = key;
		row.auto = false;
		draft = draft;
	}
	function move(index: number, by: -1 | 1) {
		const to = index + by;
		if (index < 0 || to < 0 || to >= draft.length) return;
		const next = [...draft];
		const [row] = next.splice(index, 1);
		if (row !== undefined) next.splice(to, 0, row);
		draft = next;
	}
	function drop(index: number) {
		draft = draft.filter((_, at) => at !== index);
	}
	function reseed() {
		draft = (stages.data?.stages ?? []).map((stage) => ({ ...stage, auto: false }));
	}
	/** The saved list again, discarding unsaved edits — after a `stale_stages` refusal. */
	async function reload() {
		await queryClient.invalidateQueries({ queryKey: ['stages', project] });
		reseed();
		error = '';
		stale = false;
	}

	async function save() {
		busy = true;
		error = '';
		stale = false;
		try {
			const saved = await replaceReviewStages(project, strip(draft), stages.data?.etag ?? '', saveKey);
			saveKey = newKey();
			queryClient.setQueryData(['stages', project], saved);
			reseed();
			toast.success('Saved the review stages');
		} catch (failure) {
			stale = failure instanceof ApiError && failure.reason === 'stale_stages';
			error = plainError(failure, 'change the review stages');
			// A retry keeps its key; a different save makes a new one.
		} finally {
			busy = false;
		}
	}
</script>

<svelte:head><title>Review stages · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="stages">
	<PageHeader title="Review stages">
		{#snippet description()}
			The sign-offs a publish request needs before it can be published, in order. With none, one approval publishes — the way it always worked.
			{#if changes}A stage marked <em>someone other than the sender</em> never counts an approval from whoever sent the request for review.{/if}
		{/snippet}
	</PageHeader>

	{#if mine.isError}
		<Problem error={mine.error} what="this project" retry={() => mine.refetch()} />
	{:else if mine.isPending || stages.isPending}
		<Skeleton class="mb-4 h-6 w-40" />
		<Skeleton class="h-40 w-full max-w-3xl rounded-xl" />
	{:else if stages.isError}
		<Problem error={stages.error} what="the review stages" retry={() => stages.refetch()} />
	{:else if !changes}
		<!-- Every member may read the list; only a schema.change role may replace it. -->
		<p class="max-w-3xl rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="stages-read-only">
			Your role can't change the review stages. This is the workflow publish requests go through.
		</p>
		{#if (stages.data?.stages ?? []).length === 0}
			<p class="mt-4 max-w-3xl text-sm text-muted-foreground" data-testid="stages-none">No review stages are set, so one approval publishes a request.</p>
		{:else}
			<ol class="mt-4 max-w-3xl overflow-hidden rounded-xl border border-border bg-card" data-testid="stages">
				{#each stages.data?.stages ?? [] as stage, index (stage.key)}
					<li class="rule-dotted flex items-center gap-3 px-4 py-3 text-sm last:border-b-0">
						<span class="text-xs text-muted-foreground tabular">{index + 1}.</span>
						<span class="font-medium" data-user-content>{stage.name}</span>
						{#if stage.different_reviewer}<Badge variant="secondary">someone other than the sender</Badge>{/if}
					</li>
				{/each}
			</ol>
		{/if}
	{:else if (stages.data?.stages ?? []).length === 0 && draft.length === 0}
		<div class="max-w-3xl">
			<EmptyState icon={ListChecksIcon} title="No review stages" testid="stages-empty">
				With none set, one approval publishes a request — the way it always worked. Add stages to make publishing need a named sign-off at each one, in any order.
				{#snippet action()}
					<Button onclick={addStage} data-testid="add-stage"><PlusIcon data-icon="inline-start" />Add a stage</Button>
				{/snippet}
			</EmptyState>
		</div>
	{:else}
		<section class="max-w-3xl" aria-label="Review stages">
			<ol class="space-y-3" data-testid="stages">
				{#each draft as row, index (index)}
					<li class="rounded-xl border border-border bg-card px-4 py-3" data-testid="stage">
						<div class="flex flex-wrap items-center gap-2">
							<span class="text-xs text-muted-foreground tabular" data-testid="stage-position">{index + 1}.</span>
							<div class="grid min-w-40 flex-1 gap-1">
								<label class="sr-only" for="stage-name-{index}">Stage {index + 1} name</label>
								<Input id="stage-name-{index}" value={row.name} oninput={(event) => nameAt(index, event.currentTarget.value)} required maxlength={100} placeholder="Editorial" data-testid="stage-name" />
							</div>
							<div class="grid min-w-32 flex-1 gap-1">
								<label class="sr-only" for="stage-key-{index}">Stage {index + 1} short name</label>
								<Input id="stage-key-{index}" class="font-mono" value={row.key} oninput={(event) => keyAt(index, event.currentTarget.value)} required maxlength={63} placeholder="editorial" data-testid="stage-key" />
							</div>
							<div class="flex items-center gap-1">
								<Button variant="ghost" size="icon-sm" onclick={() => move(index, -1)} disabled={index === 0 || busy} aria-label="Move {row.name || `stage ${index + 1}`} up" data-testid="stage-up"><ArrowUpIcon /></Button>
								<Button variant="ghost" size="icon-sm" onclick={() => move(index, 1)} disabled={index === draft.length - 1 || busy} aria-label="Move {row.name || `stage ${index + 1}`} down" data-testid="stage-down"><ArrowDownIcon /></Button>
								<Button variant="ghost" size="icon-sm" onclick={() => drop(index)} disabled={busy} aria-label="Remove {row.name || `stage ${index + 1}`}" data-testid="stage-remove"><XIcon /></Button>
							</div>
						</div>
						<label class="mt-2 flex items-center gap-2 text-sm text-muted-foreground">
							<input type="checkbox" class="size-4 accent-primary" bind:checked={row.different_reviewer} data-testid="stage-different" />
							Someone other than the sender must approve
						</label>
					</li>
				{/each}
			</ol>
			<div class="mt-4 flex flex-wrap items-center gap-3">
				<Button variant="outline" size="sm" onclick={addStage} disabled={busy || draft.length >= 20} data-testid="add-stage"><PlusIcon data-icon="inline-start" />Add a stage</Button>
				<span class="text-xs text-muted-foreground">A stage's short name is what a review names; changing it makes a different stage, and approvals recorded for the old one stop counting.</span>
			</div>
			{#if rowProblem !== ''}<p class="mt-3 text-sm text-muted-foreground" data-testid="stages-problem">{rowProblem}</p>{/if}
			{#if error}
				<div role="alert" class="mt-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive" data-testid="stages-error">
					<p>{error}</p>
					{#if stale}<Button class="mt-2" variant="secondary" size="sm" onclick={reload} data-testid="stages-reload">Load the latest list</Button>{/if}
				</div>
			{/if}
			<div class="mt-6 flex justify-end">
				<Button onclick={save} disabled={busy || !dirty || rowProblem !== '' || draft.length === 0} data-testid="save-stages">{busy ? 'Saving…' : 'Save stages'}</Button>
			</div>
			<p class="mt-3 text-xs text-muted-foreground">Saving replaces the whole list. Removing a stage drops its gate; reviews recorded for a stage that keeps its short name still count. Saving an empty list is allowed — one approval publishes again.</p>
		</section>
	{/if}

	<TechnicalDetails class="mt-8 max-w-3xl">
		<p>
			The API calls this the environment's <em>review stages</em>: GET answers the list with an ETag over it, PUT replaces it whole under
			<code>schema.change</code> with <code>If-Match</code> and an idempotency key.
		</p>
		<p class="font-mono break-all">/api/v1/projects/{project}/environments/production/review-stages</p>
	</TechnicalDetails>
</SettingsFrame>
