<!--
  A project's audience (D-137): its site visitors — a different credential kind from Members,
  who are staff. The page shows who has an account and which audience role it holds, lets people
  whose role manages the audience change roles or disable an account, and lists the recent
  sign-in activity the audit log records for every visitor event (D-142).
-->
<script lang="ts">
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Table from '@colophon/ui/ui/table';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import UsersRoundIcon from '@lucide/svelte/icons/users-round';
	import {
		getProject,
		listAudienceRoles,
		listAudit,
		listSiteUsers,
		may,
		updateSiteUser
	} from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => ({ queryKey: ['project', project], queryFn: () => getProject(project) }));
	const manages = $derived(may(mine.data, 'audience.manage'));
	const readsLog = $derived(may(mine.data, 'audit.read'));

	const users = createQuery(() => ({ queryKey: ['audience-users', project], queryFn: () => listSiteUsers(project), enabled: manages }));
	const roles = createQuery(() => ({ queryKey: ['audience-roles', project], queryFn: () => listAudienceRoles(project), enabled: manages }));
	const activity = createQuery(() => ({
		queryKey: ['audience-activity', project],
		queryFn: () => listAudit(project, { limit: 100 }),
		enabled: readsLog
	}));
	// `action` filters exactly, so the page keeps the events that belong to the audience
	// surface — visitor sign-ins and the staff changes that manage them.
	const events = $derived((activity.data?.data ?? []).filter((event) => event.action.startsWith('audience.')));

	let error = $state('');
	let busy = $state(false);

	const WORDS: Record<string, string> = {
		'audience.user_signed_up': 'signed up',
		'audience.user_signed_in': 'signed in',
		'audience.user_signed_out': 'signed out',
		'audience.magic_link_sent': 'was sent a sign-in link',
		'audience.magic_link_spent': 'signed in by link',
		'audience.role_created': 'created the role',
		'audience.role_updated': 'changed the role',
		'audience.role_deleted': 'deleted the role',
		'audience.user_created': 'was added',
		'audience.user_updated': 'was changed',
		'audience.session_revoked': 'had a session revoked'
	};
	const words = (action: string) => WORDS[action] ?? action;
	// The subject is the site user's id for account events, the email for a sent link; the
	// detail carries the email either way, so that is what a person reading the log wants.
	const who = (event: { subject: string; detail: Record<string, unknown> }) =>
		typeof event.detail.email === 'string' ? event.detail.email : event.subject;

	async function act(work: () => Promise<void>, doing: string) {
		busy = true;
		error = '';
		try {
			await work();
			await queryClient.invalidateQueries({ queryKey: ['audience-users', project] });
			await queryClient.invalidateQueries({ queryKey: ['audience-activity', project] });
		} catch (failure) {
			error = plainError(failure, doing);
		} finally {
			busy = false;
		}
	}

	async function setRole(select: HTMLSelectElement, user: { id: string; role: string }) {
		const was = user.role;
		await act(() => updateSiteUser(project, user.id, { role: select.value }), 'change that role');
		if (error) select.value = was;
	}

	async function toggle(user: { id: string; disabled: boolean }) {
		await act(
			() => updateSiteUser(project, user.id, { disabled: !user.disabled }),
			user.disabled ? 're-enable that account' : 'disable that account'
		);
	}

	const select =
		'h-8 rounded-md border border-input bg-card px-2 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40 disabled:opacity-50';
</script>

<svelte:head><title>Audience · Colophon Studio</title></svelte:head>

<PageHeader title="Audience">
	{#snippet description()}The project's site visitors — who has an account, which role it holds, and how people signed in.{/snippet}
</PageHeader>

{#if error}<p role="alert" class="mb-4 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive">{error}</p>{/if}

{#if mine.isPending}
	<p class="text-sm text-muted-foreground">Loading…</p>
{:else if !manages}
	<EmptyState icon={UsersRoundIcon} title="Not for this role" testid="audience-hidden">
		Your role can't see the project's site visitors.
	</EmptyState>
{:else if users.isPending}
	<p class="text-sm text-muted-foreground">Loading…</p>
{:else if users.isError}
	<Problem error={users.error} />
{:else if (users.data?.data.length ?? 0) === 0}
	<EmptyState icon={UsersRoundIcon} title="No site visitors yet" testid="audience-empty">
		When someone signs up or uses a sign-in link on the site, their account appears here.
	</EmptyState>
{:else}
	<div class="overflow-hidden rounded-xl border border-border bg-card">
		<Table.Root>
			<Table.Header>
				<Table.Row class="hover:bg-transparent">
					<Table.Head class="pl-4">Visitor</Table.Head>
					<Table.Head>Role</Table.Head>
					<Table.Head class="hidden md:table-cell">Last sign-in</Table.Head>
					<Table.Head class="hidden md:table-cell">Joined</Table.Head>
					<Table.Head>Status</Table.Head>
					<Table.Head class="pr-4 text-right"><span class="sr-only">Actions</span></Table.Head>
				</Table.Row>
			</Table.Header>
			<Table.Body data-testid="audience-users">
				{#each users.data?.data ?? [] as user (user.id)}
					<Table.Row data-testid="audience-user">
						<Table.Cell class="pl-4">
							<span class="font-medium">{user.email}</span>
							{#if user.display_name}<span class="ml-2 text-sm text-muted-foreground">{user.display_name}</span>{/if}
						</Table.Cell>
						<Table.Cell>
							{#if manages}
								<select
									class={select}
									aria-label="Role of {user.email}"
									value={user.role}
									disabled={busy}
									onchange={(event) => setRole(event.currentTarget, user)}
								>
									{#each roles.data ?? [] as option (option.name)}<option value={option.name}>{option.name}</option>{/each}
								</select>
							{:else}
								<span class="text-sm">{user.role}</span>
							{/if}
						</Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground md:table-cell">
							{user.last_sign_in_at ? new Date(user.last_sign_in_at).toLocaleString() : '—'}
						</Table.Cell>
						<Table.Cell class="hidden text-sm text-muted-foreground md:table-cell">{new Date(user.created_at).toLocaleDateString()}</Table.Cell>
						<Table.Cell class="text-sm">{user.disabled ? 'Disabled' : 'Active'}</Table.Cell>
						<Table.Cell class="pr-4 text-right">
							{#if manages}
								<Button variant="ghost" size="sm" disabled={busy} onclick={() => toggle(user)} aria-label="{user.disabled ? 'Enable' : 'Disable'} {user.email}">
									{user.disabled ? 'Enable' : 'Disable'}
								</Button>
							{/if}
						</Table.Cell>
					</Table.Row>
				{/each}
			</Table.Body>
		</Table.Root>
	</div>
{/if}

{#if manages && (roles.data ?? []).length > 0}
	<section class="mt-10 max-w-2xl" aria-labelledby="roles-heading">
		<h2 id="roles-heading" class="mb-3 text-base font-semibold">Audience roles</h2>
		<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="audience-roles">
			{#each roles.data ?? [] as role (role.name)}
				<li class="rule-dotted flex items-center gap-x-4 px-4 py-3 last:border-b-0">
					<span class="min-w-0 flex-1 font-medium">{role.name}</span>
					<span class="text-sm text-muted-foreground">{role.self_assign ? 'open to signup' : 'staff-assigned'}</span>
					<span class="text-xs text-muted-foreground">{new Date(role.created_at).toLocaleDateString()}</span>
				</li>
			{/each}
		</ul>
	</section>
{/if}

{#if readsLog}
	<section class="mt-10 max-w-2xl" aria-labelledby="activity-heading">
		<h2 id="activity-heading" class="mb-3 text-base font-semibold">Sign-in activity</h2>
		{#if activity.isPending}
			<p class="text-sm text-muted-foreground">Loading…</p>
		{:else if activity.isError}
			<Problem error={activity.error} />
		{:else}
			<ul class="overflow-hidden rounded-xl border border-border bg-card" data-testid="audience-activity">
				{#each events as event (event.id)}
					<li class="rule-dotted flex flex-wrap items-baseline gap-x-2 px-4 py-3 last:border-b-0 text-sm">
						<span class="font-medium">{who(event)}</span>
						<span class="text-muted-foreground">{words(event.action)}</span>
						<span class="ml-auto text-xs text-muted-foreground">{new Date(event.at).toLocaleString()}</span>
					</li>
				{:else}
					<li class="px-4 py-6 text-center text-sm text-muted-foreground">No sign-ins yet. The feed records them as they happen.</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}
