<!--
  A project's members (D-042, D-058). Everyone sees who belongs and with which role, and can
  leave; people whose role manages members also change roles, remove people, and invite with a
  link that is shown only once. The server refuses giving or taking away a role with powers their
  own role lacks, and the message says so.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import UserPlusIcon from '@lucide/svelte/icons/user-plus';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import * as Table from '@colophon/ui/ui/table';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import {
		ApiError,
		changeRole,
		createInvitation,
		getProject,
		type Invitation,
		listInvitations,
		listMembers,
		me,
		may,
		removeMember,
		revokeInvitation,
		type Role
	} from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';
	import { rolesQuery } from '#lib/queries.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	// The role comes from the project itself: me().projects lists at most 1,000 projects.
	const mine = createQuery(() => ({ queryKey: ['project', project], queryFn: () => getProject(project) }));
	const manages = $derived(may(mine.data, 'members.manage'));
	const roles = createQuery(() => rolesQuery(project));
	const nameOf = (key: string) => roles.data?.find((option) => option.key === key)?.name ?? key;
	const people = createQuery(() => ({ queryKey: ['members', project], queryFn: () => listMembers(project) }));
	const pending = createQuery(() => ({
		queryKey: ['invitations', project],
		queryFn: () => listInvitations(project),
		enabled: manages
	}));

	let error = $state('');
	let busy = $state(false);
	let label = $state('');
	let role = $state<Role>('editor');
	// The invitation just made: its link exists only in this answer.
	let made = $state<Invitation | null>(null);
	let copied = $state(false);

	async function act(work: () => Promise<void>, doing = 'make this change'): Promise<boolean> {
		busy = true;
		error = '';
		try {
			await work();
			return true;
		} catch (failure) {
			error = plainError(failure, doing);
			return false;
		} finally {
			busy = false;
		}
	}

	const refresh = () =>
		Promise.all([
			queryClient.invalidateQueries({ queryKey: ['members', project] }),
			queryClient.invalidateQueries({ queryKey: ['invitations', project] }),
			queryClient.invalidateQueries({ queryKey: ['project', project] }),
			queryClient.invalidateQueries({ queryKey: ['me'] })
		]);

	async function setRole(select: HTMLSelectElement, identity: string, was: Role) {
		const next = select.value as Role;
		if (!(await act(() => changeRole(project, identity, next), 'give that role'))) select.value = was;
		await refresh();
	}

	async function remove(identity: string) {
		await act(() => removeMember(project, identity), 'remove people');
		await refresh();
	}

	async function leave() {
		const id = who.data?.id;
		if (id !== undefined && (await act(() => removeMember(project, id)))) {
			await queryClient.invalidateQueries({ queryKey: ['me'] });
			await goto(resolve('/'));
		}
	}

	async function invite(event: SubmitEvent) {
		event.preventDefault();
		made = null;
		copied = false;
		await act(async () => {
			made = await createInvitation(project, role, label);
			label = '';
		}, 'invite people with that role');
		await refresh();
	}

	const select =
		'h-8 rounded-md border border-input bg-card px-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40 disabled:opacity-50';
	const initials = (name: string) =>
		name
			.split(/\s+/)
			.slice(0, 2)
			.map((word) => word.slice(0, 1).toUpperCase())
			.join('');

	async function copy() {
		if (made?.link === undefined) return;
		try {
			await navigator.clipboard.writeText(made.link);
			copied = true;
		} catch {
			error = "Couldn't copy; select the link and copy it yourself.";
		}
	}
</script>

<svelte:head><title>Members · Colophon Studio</title></svelte:head>

<PageHeader title="Members">
	{#snippet description()}Who belongs to this project, and with which role. People who manage members invite others with a link that works once.{/snippet}
</PageHeader>

{#if error}<p role="alert" class="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive" data-testid="members-error">{error}</p>{/if}

{#if people.isPending || who.isPending}
	<p class="text-sm text-muted-foreground">Loading…</p>
{:else if people.isError}
	<Problem error={people.error} />
{:else}
	<div class="overflow-hidden rounded-xl border border-border bg-card">
		<Table.Root>
			<Table.Header>
				<Table.Row class="hover:bg-transparent">
					<Table.Head class="pl-4">Name</Table.Head>
					<Table.Head>Role</Table.Head>
					<Table.Head class="hidden md:table-cell">Joined</Table.Head>
					<Table.Head class="pr-4 text-right"><span class="sr-only">Actions</span></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body data-testid="members">
				{#each people.data as member (member.identity_id)}
					{@const name = member.display_name ?? member.identity_id}
					{@const self = member.identity_id === who.data?.id}
					<Table.Row data-testid="member">
						<Table.Cell class="pl-4">
							<span class="flex items-center gap-3">
								<span class="flex size-8 shrink-0 items-center justify-center rounded-full bg-secondary text-xs font-medium" aria-hidden="true">{initials(name)}</span>
								<span class="font-medium">{name}{#if self}&nbsp;<span class="font-normal text-muted-foreground">(you)</span>{/if}</span>
							</span>
						</Table.Cell>
						<Table.Cell>
							{#if manages}
								<select
									class={select}
									aria-label="Role of {name}"
									value={member.role}
									disabled={busy}
									onchange={(event) => setRole(event.currentTarget, member.identity_id, member.role)}
								>
									{#each roles.data ?? [] as option (option.key)}<option value={option.key}>{option.name}</option>{/each}
								</select>
							{:else}
								<span class="text-sm">{nameOf(member.role)}</span>
							{/if}
						</Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground md:table-cell">{new Date(member.joined_at).toLocaleDateString()}</Table.Cell>
						<Table.Cell class="pr-4 text-right">
							{#if manages && !self}
								<Button variant="ghost" size="sm" disabled={busy} onclick={() => remove(member.identity_id)} aria-label="Remove {name}">Remove</Button>
							{/if}
							{#if self}<Button variant="ghost" size="sm" disabled={busy} onclick={leave}>Leave project</Button>{/if}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}

{#if people.data && people.data.length === 1 && (pending.data ?? []).length === 0}
	<div class="mt-6">
		<EmptyState icon={UserPlusIcon} title="It's just you so far" testid="members-alone">
			{#if manages}
				Invite the people you write with. Each gets a link that works once, with the role you choose: Editor to write, Publisher to approve and
				publish.
			{:else}
				Someone who manages members can invite more people.
			{/if}
			{#snippet action()}
				{#if manages}<Button variant="outline" onclick={() => document.getElementById('invite-label')?.focus()}>Invite someone</Button>{/if}
			{/snippet}
		</EmptyState>
	</div>
{/if}

{#if manages}
	<section class="mt-12 max-w-4xl scroll-mt-20" aria-labelledby="invite-heading" id="invite">
		<h2 id="invite-heading" class="mb-1 text-base font-semibold">Invite someone</h2>
		<p class="mb-4 text-sm text-muted-foreground">The link is shown once, works once, and expires on its own.</p>
		<form onsubmit={invite} aria-label="Invite someone">
			<FormRow label="Who is it for?" hint="Only you see this, to recognise the invitation." for="invite-label">
				<Input id="invite-label" class="max-w-md" bind:value={label} maxlength={100} required placeholder="Sam, copy editor" />
			</FormRow>
			<FormRow label="Role" hint={roles.data?.find((option) => option.key === role)?.description} for="invite-role">
				<select id="invite-role" class="{select} h-9 w-full max-w-md" bind:value={role}>
					{#each roles.data ?? [] as option (option.key)}<option value={option.key}>{option.name}</option>{/each}
				</select>
			</FormRow>
			<div class="flex justify-end"><Button type="submit" disabled={busy}>Create invitation link</Button></div>
		</form>

		{#if made?.link}
			<section class="mt-6 rounded-xl border border-live/40 bg-live-soft/50 p-4" data-testid="invitation-link" aria-label="Invitation link">
				<p class="text-sm">
					Send this link to <strong>{made.label}</strong>. It is shown only now, works once, and expires {new Date(made.expires_at).toLocaleString()}.
				</p>
				<div class="mt-3 flex gap-2">
					<Input readonly value={made.link} aria-label="Invitation link" class="font-mono text-xs" onfocus={(event) => event.currentTarget.select()} />
					<Button variant="secondary" onclick={copy}>{copied ? 'Copied' : 'Copy'}</Button>
				</div>
			</section>
		{/if}

		<h2 class="mt-10 mb-3 text-base font-semibold">Pending invitations</h2>
		{#if pending.isPending}
			<p class="text-sm text-muted-foreground">Loading…</p>
		{:else if pending.isError}
			<Problem error={pending.error} />
		{:else}
			<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="invitations">
				{#each pending.data as invitation (invitation.id)}
					<li class="rule-dotted flex flex-wrap items-center gap-x-4 gap-y-1 px-4 py-3 last:border-b-0">
						<span class="min-w-0 flex-1 font-medium">{invitation.label}</span>
						<span class="text-sm">{nameOf(invitation.role)}</span>
						<span class="text-xs text-muted-foreground">until {new Date(invitation.expires_at).toLocaleString()}</span>
						<Button
							variant="ghost"
							size="sm"
							disabled={busy}
							onclick={() => act(() => revokeInvitation(project, invitation.id), 'revoke invitations').then(refresh)}
							aria-label="Revoke the invitation for {invitation.label}">Revoke</Button
						>
					</li>
				{:else}
					<li class="px-4 py-6 text-center text-sm text-muted-foreground">None. Links you make appear here until they're used, revoked or expire.</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}
