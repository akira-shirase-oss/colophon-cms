<!--
  One role (D-058): what it may do, who holds it, and, for people whose role manages roles, the
  editor. Every member can read it, so anyone can see what a role means before it's given to them.
  It sits in Settings' frame, under Roles (D-081). Made from Members' "Make a new role…" (`for=invite`,
  D-082), it says so, and saving goes back to the invitation with this role chosen.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery } from '@tanstack/svelte-query';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { Button } from '@colophon/ui/ui/button';
	import { may } from '#lib/api.ts';
	import { backToInvite } from '#lib/inviteDraft.ts';
	import { projectQuery, rolesQuery, typesQuery } from '#lib/queries.ts';
	import RoleEditor from '#lib/RoleEditor.svelte';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const key = $derived(page.params.role ?? '');
	const mine = createQuery(() => projectQuery(project));
	const roles = createQuery(() => rolesQuery(project));
	const types = createQuery(() => typesQuery(project));
	const role = $derived(roles.data?.find((item) => item.key === key));
	const editable = $derived(may(mine.data, 'roles.manage') && role !== undefined && !role.fixed);
	// Every query's data is read on every pass, never behind `&&`: a query's result only tells this
	// page about the fields it has read, so one read only after another arrived would never update.
	const loaded = $derived([roles.data, types.data, mine.data].every((data) => data !== undefined));
	// Made for an invitation: back there, with this role chosen.
	const forInvite = $derived(page.url.searchParams.get('for') === 'invite');
	const invite = $derived(backToInvite(resolve('/projects/[project]/members', { project }), key));
</script>

<svelte:head><title>{role?.name ?? 'Role'} · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="roles">
	<PageHeader title={role?.name ?? 'Role'}>
		{#snippet meta()}
			{#if role}
				<span>{role.members} {role.members === 1 ? 'person holds' : 'people hold'} it</span>
				<span>Short name <code class="font-mono">{role.key}</code></span>
				<span>Version {role.version}</span>
			{/if}
		{/snippet}
	</PageHeader>

	{#if roles.isError}
		<Problem error={roles.error} />
	{:else if types.isError}
		<Problem error={types.error} />
	{:else if loaded && types.data}
		{#if role}
			{#if forInvite}
				<div class="mb-6 flex max-w-4xl flex-wrap items-center justify-between gap-3 rounded-xl border border-border bg-card px-4 py-3 text-sm" data-testid="role-for-invite">
					<p>Choose what <strong class="font-medium" data-user-content>{role.name}</strong> may do, then save. You'll go back to your invitation with it chosen.</p>
					<Button variant="outline" size="sm" href={invite}>Back to the invitation</Button>
				</div>
			{/if}
			{#key `${role.key}@${role.version}`}
				<RoleEditor {project} {role} types={types.data} {editable} onsaved={forInvite ? () => goto(invite) : undefined} />
			{/key}
		{:else}
			<p class="rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="role-missing">
				This project has no role called <code class="font-mono">{key}</code>. It may have been deleted.
			</p>
		{/if}
	{:else}
		<div class="grid max-w-4xl gap-3" aria-busy="true">
			<Skeleton class="h-10 w-80" />
			<Skeleton class="h-48 w-full" />
		</div>
	{/if}
</SettingsFrame>
