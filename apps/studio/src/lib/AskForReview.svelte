<!--
  Ask for review, with who should look (the audit's P2-12): the request's one primary while it is
  prepared opens a small form that offers the members whose role may approve everything in it
  (those who may also publish first), an optional note, and Send for review. Sending submits the
  request on its hash, then gives each chosen reviewer a task bound to the request (E1, D-102),
  which their Your work and Tasks pages list, and which the request page reads back as "Waiting
  for …". A task that can't be made is said, and the request stays sent.
-->
<script lang="ts">
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Label } from '@colophon/ui/ui/label';
	import * as Popover from '@colophon/ui/ui/popover';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { type Changeset, changesetAction, createTask, may, newKey } from './api.ts';
	import { plainError } from './errors.ts';
	import { meQuery, membersQuery, projectQuery, rolesQuery } from './queries.ts';
	import { ACTION, names, reviewTaskTitle } from './requestState.ts';

	let {
		project,
		changeset,
		disabled = false,
		onsent,
		onerror
	}: {
		project: string;
		changeset: Changeset;
		disabled?: boolean;
		/** The request as the server answered the submit. */
		onsent: (next: Changeset, warning: string) => void;
		onerror: (failure: unknown) => void;
	} = $props();

	const queryClient = useQueryClient();
	let open = $state(false);
	const members = createQuery(() => ({ ...membersQuery(project), enabled: open }));
	const roles = createQuery(() => ({ ...rolesQuery(project), enabled: open }));
	const who = createQuery(meQuery);
	const mine = createQuery(() => projectQuery(project));
	const collections = $derived([...new Set(changeset.items.map((item) => item.collection))]);
	/** What a member's role allows, read from the project's roles (any member may read them). */
	const grantsOf = (role: string) => ({ permissions: roles.data?.find((one) => one.key === role)?.permissions ?? [] });
	const everywhere = (role: string, action: 'content.review' | 'content.publish') => collections.every((collection) => may(grantsOf(role), action, collection));
	// Everyone else who may approve all of it; those who may also publish it come first.
	const reviewers = $derived(
		(members.data ?? [])
			.filter((member) => member.identity_id !== who.data?.id && everywhere(member.role, 'content.review'))
			.map((member) => ({ ...member, publishes: everywhere(member.role, 'content.publish') }))
			.sort((one, other) => Number(other.publishes) - Number(one.publishes) || (one.display_name ?? '').localeCompare(other.display_name ?? ''))
	);
	const loading = $derived(open && (members.isPending || roles.isPending));
	const selfApproves = $derived(mine.data?.self_review === true && collections.every((collection) => may(mine.data, 'content.review', collection)));

	let picked = $state<string[]>([]);
	let note = $state('');
	let busy = $state(false);

	async function send(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		try {
			const next = await changesetAction(project, changeset.id, changeset.hash, 'submit');
			// The request is sent; each reviewer's task is said on its own if it can't be made.
			const first = changeset.items[0];
			const unasked: string[] = [];
			if (first !== undefined) {
				for (const assignee of picked) {
					try {
						await createTask(project, { entry: first.entry_id, title: reviewTaskTitle(changeset.title, note), changeset: changeset.id, assignee }, newKey());
					} catch {
						unasked.push(reviewers.find((member) => member.identity_id === assignee)?.display_name ?? 'someone');
					}
				}
			}
			await queryClient.invalidateQueries({ queryKey: ['tasks', project] });
			open = false;
			picked = [];
			note = '';
			onsent(next, unasked.length === 0 ? '' : `It's sent for review, but ${names(unasked)} couldn't be told. Ask them on the Tasks page.`);
		} catch (failure) {
			open = false;
			onerror(failure);
		} finally {
			busy = false;
		}
	}
</script>

<Popover.Root bind:open>
	<Popover.Trigger>
		{#snippet child({ props })}
			<Button {...props} disabled={disabled || busy} data-testid="ask-review">{ACTION.askReview}</Button>
		{/snippet}
	</Popover.Trigger>
	<Popover.Content align="end" class="w-[min(22rem,calc(100vw-2rem))]" data-testid="ask-review-form">
		<form class="grid gap-4" onsubmit={send} aria-label="Ask for review">
			<fieldset class="grid gap-2">
				<legend class="mb-1 text-sm font-medium">Who should review it?</legend>
				{#if loading}
					<p class="text-sm text-muted-foreground">Finding who can approve it…</p>
				{:else if members.isError || roles.isError}
					<p class="text-sm text-muted-foreground">Couldn't load who can approve it. You can still send it; anyone who can approve will find it in Publish requests.</p>
				{:else if reviewers.length === 0}
					<p class="text-sm text-muted-foreground" data-testid="no-reviewers">
						Nobody else here can approve it.{#if selfApproves}
							You can approve it yourself once it's sent.{/if}
					</p>
				{:else}
					<ul class="grid max-h-56 gap-1 overflow-y-auto">
						{#each reviewers as member (member.identity_id)}
							<li>
								<label class="flex items-center gap-2.5 rounded-control px-2 py-1.5 text-sm hover:bg-muted">
									<input type="checkbox" class="size-4" value={member.identity_id} bind:group={picked} />
									<span class="min-w-0 flex-1 truncate" data-user-content>{member.display_name ?? 'A member'}</span>
									<span class="shrink-0 text-xs text-muted-foreground">{member.publishes ? 'Can publish' : 'Can approve'}</span>
								</label>
							</li>
						{/each}
					</ul>
				{/if}
			</fieldset>
			<div class="grid gap-1.5">
				<Label for="review-note">Note (optional)</Label>
				<Textarea id="review-note" bind:value={note} rows={2} maxlength={120} placeholder="What to look at" />
			</div>
			<Button type="submit" disabled={busy}>{busy ? 'Sending…' : 'Send for review'}</Button>
		</form>
	</Popover.Content>
</Popover.Root>
