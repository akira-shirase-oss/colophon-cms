<!--
  A project's members (D-042). Everyone sees who belongs and with which role, and can leave;
  owners also change roles, remove people, and invite with a link that is shown only once.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button, TextField } from '@colophon/ui';
	import {
		ApiError,
		changeRole,
		createInvitation,
		type Invitation,
		listInvitations,
		listMembers,
		me,
		removeMember,
		revokeInvitation,
		type Role,
		ROLE_SUMMARY,
		ROLES
	} from '#lib/api.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const owner = $derived(who.data?.projects.find((mine) => mine.id === project)?.role === 'owner');
	const people = createQuery(() => ({ queryKey: ['members', project], queryFn: () => listMembers(project) }));
	const pending = createQuery(() => ({
		queryKey: ['invitations', project],
		queryFn: () => listInvitations(project),
		enabled: owner
	}));

	let error = $state('');
	let busy = $state(false);
	let label = $state('');
	let role = $state<Role>('editor');
	// The invitation just made: its link exists only in this answer.
	let made = $state<Invitation | null>(null);
	let copied = $state(false);

	async function act(work: () => Promise<void>): Promise<boolean> {
		busy = true;
		error = '';
		try {
			await work();
			return true;
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
			return false;
		} finally {
			busy = false;
		}
	}

	const refresh = () =>
		Promise.all([
			queryClient.invalidateQueries({ queryKey: ['members', project] }),
			queryClient.invalidateQueries({ queryKey: ['invitations', project] }),
			queryClient.invalidateQueries({ queryKey: ['me'] })
		]);

	async function setRole(select: HTMLSelectElement, identity: string, was: Role) {
		const next = select.value as Role;
		if (!(await act(() => changeRole(project, identity, next)))) select.value = was;
		await refresh();
	}

	async function remove(identity: string) {
		await act(() => removeMember(project, identity));
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
		});
		await refresh();
	}

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

<p><a href={resolve('/projects/[project]', { project })}>← Content</a></p>
<h1>Members</h1>

{#if error}<p role="alert" class="error" data-testid="members-error">{error}</p>{/if}

{#if people.isPending || who.isPending}
	<p class="muted">Loading…</p>
{:else if people.isError}
	<p role="alert">{people.error.message}</p>
{:else}
	<ul class="people" data-testid="members">
		{#each people.data as member (member.identity_id)}
			{@const name = member.display_name ?? member.identity_id}
			{@const self = member.identity_id === who.data?.id}
			<li data-testid="member">
				<span class="name">{name}{#if self}<small class="muted"> (you)</small>{/if}</span>
				{#if owner}
					<select
						aria-label="Role of {name}"
						value={member.role}
						disabled={busy}
						onchange={(event) => setRole(event.currentTarget, member.identity_id, member.role)}
					>
						{#each ROLES as option (option)}<option value={option}>{option}</option>{/each}
					</select>
					{#if !self}
						<Button variant="text" disabled={busy} onclick={() => remove(member.identity_id)} aria-label="Remove {name}">Remove</Button>
					{/if}
				{:else}
					<span class="role">{member.role}</span>
				{/if}
				{#if self}<Button variant="text" disabled={busy} onclick={leave}>Leave project</Button>{/if}
			</li>
		{/each}
	</ul>
{/if}

{#if owner}
	<form class="invite" onsubmit={invite} aria-label="Invite someone">
		<h2>Invite someone</h2>
		<TextField label="Who is it for?" bind:value={label} maxlength={100} required placeholder="Sam, copy editor" />
		<div class="choice">
			<label for="invite-role">Role</label>
			<select id="invite-role" bind:value={role}>
				{#each ROLES as option (option)}<option value={option}>{option}: {ROLE_SUMMARY[option]}</option>{/each}
			</select>
		</div>
		<div><Button type="submit" disabled={busy}>Create invitation link</Button></div>
	</form>

	{#if made?.link}
		<section class="made" data-testid="invitation-link" aria-label="Invitation link">
			<p>Send this link to <strong>{made.label}</strong>. It is shown only now, works once, and expires {new Date(made.expires_at).toLocaleString()}.</p>
			<div class="copy">
				<input readonly value={made.link} aria-label="Invitation link" onfocus={(event) => event.currentTarget.select()} />
				<Button variant="tonal" onclick={copy}>{copied ? 'Copied' : 'Copy'}</Button>
			</div>
		</section>
	{/if}

	<h2>Pending invitations</h2>
	{#if pending.isPending}
		<p class="muted">Loading…</p>
	{:else if pending.isError}
		<p role="alert">{pending.error.message}</p>
	{:else}
		<ul class="people" data-testid="invitations">
			{#each pending.data as invitation (invitation.id)}
				<li>
					<span class="name">{invitation.label}</span>
					<span class="role">{invitation.role}</span>
					<small class="muted">until {new Date(invitation.expires_at).toLocaleString()}</small>
					<Button variant="text" disabled={busy} onclick={() => act(() => revokeInvitation(project, invitation.id)).then(refresh)} aria-label="Revoke the invitation for {invitation.label}">Revoke</Button>
				</li>
			{:else}
				<li class="muted">None.</li>
			{/each}
		</ul>
	{/if}
{/if}

<style>
	.people {
		display: grid;
		gap: 0.5rem;
		padding: 0;
		list-style: none;
		max-width: 44rem;
	}
	.people li {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.75rem;
	}
	.name {
		flex: 1 1 12rem;
	}
	.role {
		font: var(--md-sys-typescale-label-large);
	}
	.invite {
		display: grid;
		gap: 0.75rem;
		max-width: 32rem;
		margin-top: 2rem;
	}
	.invite h2 {
		margin: 0;
	}
	.choice {
		display: grid;
		gap: 0.25rem;
		font: var(--md-sys-typescale-body-small);
		color: var(--md-sys-color-on-surface-variant);
	}
	select,
	.copy input {
		font: var(--md-sys-typescale-body-large);
		color: var(--md-sys-color-on-surface);
		background: transparent;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-extra-small, 4px);
		padding: 0.5rem 0.75rem;
	}
	.made {
		max-width: 44rem;
		margin-top: 1rem;
	}
	.copy {
		display: flex;
		gap: 0.5rem;
	}
	.copy input {
		flex: 1;
		min-width: 0;
	}
	.error {
		color: var(--md-sys-color-error);
	}
	.muted {
		color: var(--md-sys-color-on-surface-variant);
	}
</style>
