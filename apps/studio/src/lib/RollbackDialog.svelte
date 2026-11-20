<!--
  Rolling back to a release, asked first (the audit's P1-2, F1). Nothing is made until the
  person has seen exactly what the rollback would do and pressed Create rollback request: the
  server's preview (`GET …/releases/{n}/rollback`, the same pins a rollback would carry) lists
  what it puts back and what it takes down. A takedown survives a rollback (D-049, D-097): an
  entry readers had in that release but that was taken down since stays down, and the dialog says
  so on its own row, with a way to publish it again. The request it makes is reviewed and
  published like any other.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { goto } from '$app/navigation';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, newKey, previewRollback, rollbackRelease } from './api.ts';
	import { plainError } from './errors.ts';
	import { typesQuery } from './queries.ts';
	import Problem from './shell/Problem.svelte';
	import StatusBadge from './shell/StatusBadge.svelte';
	import { count, PUBLISH_REQUEST, releaseName } from './words.ts';

	let { project, generation, open = $bindable(false) }: { project: string; generation: number; open?: boolean } = $props();

	const preview = createQuery(() => ({
		queryKey: ['rollback-preview', project, generation],
		queryFn: () => previewRollback(project, generation),
		enabled: open,
		// Always what the server would build now: a release or an edit since changes it.
		staleTime: 0,
		gcTime: 0
	}));
	const types = createQuery(() => typesQuery(project));
	const typeName = (collection: string) => types.data?.find((type) => type.slug === collection)?.definition.name ?? collection;
	const back = $derived((preview.data?.restore ?? []).filter((line) => line.kind === 'publish'));
	const down = $derived((preview.data?.restore ?? []).filter((line) => line.kind === 'unpublish'));
	const kept = $derived(preview.data?.kept_down ?? []);
	// The server's totals, not the listed rows: it lists at most 100, and none this role can't read.
	const backTotal = $derived(preview.data?.publish_total ?? 0);
	const downTotal = $derived(preview.data?.unpublish_total ?? 0);
	const nothing = $derived(preview.data !== undefined && backTotal + downTotal === 0);
	const tooLarge = $derived(preview.data?.too_large ?? false);
	const summary = $derived(
		[
			...(backTotal > 0 ? [`puts back ${count(backTotal, 'entry', 'entries')}`] : []),
			...(downTotal > 0 ? [`takes down ${count(downTotal, 'entry', 'entries')}`] : [])
		].join(' and ')
	);
	const more = (total: number, listed: number) => (total > listed ? `And ${total - listed} more` : '');
	const entryHref = (collection: string, entry: string) => resolve('/projects/[project]/[collection]/[entry]', { project, collection, entry });

	// One key per dialog: pressing Create again after a lost answer makes one request, not two.
	let key = newKey();
	let busy = $state(false);
	let error = $state('');
	// Set when the confirm was refused because something was published while the dialog was open:
	// the preview is fetched again, and the person confirms what it says now.
	let moved = $state(false);
	$effect(() => {
		if (!open) {
			error = '';
			moved = false;
			key = newKey();
		}
	});

	async function create() {
		const seen = preview.data;
		if (seen === undefined || seen.hash === null || busy) return;
		busy = true;
		error = '';
		moved = false;
		try {
			// The hash of what this dialog shows: the server makes the request only if it would pin
			// exactly that (D-185).
			const made = await rollbackRelease(project, generation, `Rollback to ${releaseName(generation)}`, key, seen.hash);
			open = false;
			await goto(resolve('/projects/[project]/changesets/[changeset]', { project, changeset: made.id }));
		} catch (failure) {
			if (failure instanceof ApiError && failure.reason === 'stale_changeset') {
				moved = true;
				key = newKey();
				await preview.refetch();
			} else {
				error = plainError(failure, 'make this rollback request');
			}
		} finally {
			busy = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-[40rem]" data-testid="rollback-preview">
		<Dialog.Header>
			<Dialog.Title>Roll back to {releaseName(generation)}?</Dialog.Title>
			<Dialog.Description>
				{#if preview.data === undefined}
					Working out what it would change for readers.
				{:else if tooLarge}
					This rollback changes {count(backTotal + downTotal, 'entry', 'entries')}; one {PUBLISH_REQUEST} can carry up to {preview.data.max_items}. Roll back to a more recent release first, then continue from there — or take entries down in smaller requests.
				{:else if nothing}
					Readers already get what {releaseName(generation)} gave them, as far as a rollback can bring back.
				{:else}
					This makes a publish request that {summary}. It goes through review like any other; readers see nothing change until it is published.
				{/if}
			</Dialog.Description>
		</Dialog.Header>

		{#if preview.isPending}
			<div class="grid gap-2" aria-busy="true">
				<Skeleton class="h-5 w-1/3" />
				<Skeleton class="h-12 w-full rounded-lg" />
				<Skeleton class="h-12 w-full rounded-lg" />
			</div>
		{:else if preview.isError}
			<Problem error={preview.error} what="what a rollback would change" retry={() => preview.refetch()} />
		{:else}
			{#if moved}
				<p role="status" class="rounded-lg border border-pending/40 bg-pending-soft/60 px-3 py-2 text-sm" data-testid="rollback-moved">
					Something was published while this was open, so the rollback changed. This is what it would do now.
					<a class="underline underline-offset-2" href={resolve('/projects/[project]/releases', { project })} data-testid="see-what-changed">See what changed</a>
				</p>
			{/if}
			<div class="grid min-w-0 gap-5">
				{#if backTotal > 0}
					<section aria-labelledby="rollback-back">
						<h3 id="rollback-back" class="mb-2 text-sm font-medium">Put back</h3>
						<ul class="overflow-hidden rounded-lg border border-border" data-testid="rollback-back">
							{#each back as line (line.entry_id)}
								<li class="rule-dotted flex items-center gap-3 px-3 py-2.5 last:border-b-0" data-entry={line.entry_id}>
									<span class="min-w-0 flex-1">
										{#if line.title_hidden}
											<span class="block truncate text-sm font-medium text-muted-foreground italic">Title hidden for your role</span>
										{:else}
											<a class="block truncate text-sm font-medium hover:underline" href={entryHref(line.collection, line.entry_id)} title={line.title} data-user-content>{line.title}</a>
										{/if}
										<span class="text-xs text-muted-foreground"
											>{typeName(line.collection)} · {line.live_revision_id === null ? 'readers get nothing of it now' : `back to its version in ${releaseName(generation)}`}</span
										>
									</span>
									<StatusBadge state="updated" text="Put back" title="Readers get its version from {releaseName(generation)} again" />
								</li>
							{/each}
						</ul>
						{#if more(backTotal, back.length)}<p class="mt-2 text-xs text-muted-foreground" data-testid="rollback-back-more">{more(backTotal, back.length)}.</p>{/if}
					</section>
				{/if}
				{#if downTotal > 0}
					<section aria-labelledby="rollback-down">
						<h3 id="rollback-down" class="mb-2 text-sm font-medium">Take down</h3>
						<ul class="overflow-hidden rounded-lg border border-border" data-testid="rollback-down">
							{#each down as line (line.entry_id)}
								<li class="rule-dotted flex items-center gap-3 px-3 py-2.5 last:border-b-0" data-entry={line.entry_id}>
									<span class="min-w-0 flex-1">
										{#if line.title_hidden}
											<span class="block truncate text-sm font-medium text-muted-foreground italic">Title hidden for your role</span>
										{:else}
											<a class="block truncate text-sm font-medium hover:underline" href={entryHref(line.collection, line.entry_id)} title={line.title} data-user-content>{line.title}</a>
										{/if}
										<span class="text-xs text-muted-foreground">{typeName(line.collection)} · readers didn't have it in {releaseName(generation)}</span>
									</span>
									<StatusBadge state="taken_down" text="Take down" title="Readers will no longer see it" />
								</li>
							{/each}
						</ul>
						{#if more(downTotal, down.length)}<p class="mt-2 text-xs text-muted-foreground" data-testid="rollback-down-more">{more(downTotal, down.length)}.</p>{/if}
					</section>
				{/if}
				{#if preview.data.kept_down_total > 0}
					<section aria-labelledby="rollback-kept">
						<h3 id="rollback-kept" class="mb-1 text-sm font-medium">Stays down</h3>
						<p class="mb-2 text-sm text-muted-foreground">A rollback never undoes a takedown. Publish these again if readers should have them.</p>
						<ul class="overflow-hidden rounded-lg border border-border" data-testid="rollback-kept">
							{#each kept as line (line.entry_id)}
								<li class="rule-dotted flex flex-wrap items-center gap-x-3 gap-y-1 px-3 py-2.5 last:border-b-0" data-entry={line.entry_id}>
									<span class="min-w-0 flex-1">
										{#if line.title_hidden}
											<span class="block truncate text-sm font-medium text-muted-foreground italic">Title hidden for your role</span>
										{:else}
											<span class="block truncate text-sm font-medium" title={line.title} data-user-content>{line.title}</span>
										{/if}
										<span class="text-xs text-muted-foreground" data-testid="kept-why"
											>Taken down after {releaseName(generation)} (in {releaseName(line.taken_down_in)}), so a rollback won't bring it back</span
										>
									</span>
									{#if line.entry_deleted}
										<span class="text-xs text-muted-foreground">Deleted since</span>
									{:else}
										<Button variant="link" size="sm" class="h-auto p-0" href={entryHref(line.collection, line.entry_id)} data-testid="publish-again">Publish it again</Button>
									{/if}
								</li>
							{/each}
						</ul>
						{#if preview.data.kept_down_total > kept.length}
							<p class="mt-2 text-xs text-muted-foreground">And {preview.data.kept_down_total - kept.length} more.</p>
						{/if}
					</section>
				{/if}
			</div>
		{/if}
		{#if error}<p role="alert" class="text-sm text-destructive" data-testid="rollback-error">{error}</p>{/if}
		<Dialog.Footer>
			<Button variant="outline" type="button" onclick={() => (open = false)}>{nothing || tooLarge ? 'Close' : 'Cancel'}</Button>
			{#if !nothing && !tooLarge}
				<Button onclick={create} disabled={busy || preview.data === undefined} data-testid="create-rollback">{busy ? 'Making the request…' : 'Create rollback request'}</Button>
			{/if}
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
