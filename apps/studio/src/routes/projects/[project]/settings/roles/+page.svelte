<!--
  Settings → Roles (D-058, D-081): what each person in the project may do. Every member may read
  them, so the list shows to anyone in Settings; New role, for roles that manage roles, opens a
  dialog that asks only for a name and the short name memberships use, then opens the new role's
  page to choose what it may do. `?new=1` opens the dialog at once; `for=invite` (Members' "Make a
  new role…", D-082) carries on to the role's page, which goes back to the invitation once saved.
-->
<script lang="ts">
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, createRole, may } from '#lib/api.ts';
	import { slugify } from '#lib/content.ts';
	import { projectQuery, rolesQuery } from '#lib/queries.ts';
	import SettingsFrame from '#lib/SettingsFrame.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const project = $derived(page.params.project ?? '');
	const queryClient = useQueryClient();
	const mine = createQuery(() => projectQuery(project));
	const managesRoles = $derived(may(mine.data, 'roles.manage'));
	const roles = createQuery(() => rolesQuery(project));

	// A new role: a name, and the key memberships use, suggested from it. It starts with no
	// permissions; the role's page sets them.
	let making = $state(false);
	// Asked for by address (`?new=1`): the dialog opens once the project says this role may.
	// svelte-ignore state_referenced_locally
	let asked = page.url.searchParams.get('new') === '1';
	const forInvite = $derived(page.url.searchParams.get('for') === 'invite');
	$effect(() => {
		if (asked && managesRoles) {
			asked = false;
			start();
		}
	});
	let roleName = $state('');
	let roleKey = $state('');
	let roleKeyTouched = $state(false);
	let roleError = $state('');
	let roleBusy = $state(false);
	const suggestedKey = $derived(
		slugify(roleName)
			.replace(/^[^a-z]+/, '')
			.slice(0, 63)
	);
	const effectiveKey = $derived(roleKeyTouched ? roleKey : suggestedKey);

	function start() {
		making = true;
		roleName = '';
		roleKey = '';
		roleKeyTouched = false;
		roleError = '';
	}

	async function addRole(event: SubmitEvent) {
		event.preventDefault();
		roleBusy = true;
		roleError = '';
		try {
			const created = await createRole(project, effectiveKey, { name: roleName.trim(), description: '', permissions: [] });
			await queryClient.invalidateQueries({ queryKey: ['roles', project] });
			toast.success(`Created ${created.name}`);
			making = false;
			const rolePage = resolve('/projects/[project]/settings/roles/[role]', { project, role: created.key });
			await goto(forInvite ? `${rolePage}?for=invite` : rolePage);
		} catch (failure) {
			roleError = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			roleBusy = false;
		}
	}
</script>

<svelte:head><title>Roles · Colophon Studio</title></svelte:head>

<SettingsFrame {project} current="roles">
	<PageHeader title="Roles">
		{#snippet description()}What each person in the project may do. Owners can always do everything; other roles can be changed, and you can make your own.{/snippet}
		{#snippet actions()}
			{#if managesRoles}
				<Button onclick={start} data-testid="new-role"><PlusIcon data-icon="inline-start" />New role</Button>
			{/if}
		{/snippet}
	</PageHeader>

	{#if roles.isError}
		<Problem error={roles.error} what="roles" retry={() => roles.refetch()} />
	{:else if roles.isPending}
		<Skeleton class="h-48 w-full max-w-3xl rounded-xl" />
	{:else}
		<ul class="max-w-3xl overflow-hidden rounded-xl border border-border bg-card" data-testid="roles">
			{#each roles.data as item (item.key)}
				<li class="rule-dotted last:border-b-0">
					<a class="flex items-center gap-4 px-4 py-3 hover:bg-accent" href={resolve('/projects/[project]/settings/roles/[role]', { project, role: item.key })}>
						<span class="min-w-0 flex-1">
							<span class="flex flex-wrap items-center gap-2">
								<strong class="font-medium" data-user-content>{item.name}</strong>
								{#if item.built_in}<Badge variant="secondary">Built in</Badge>{/if}
							</span>
							{#if item.description}<span class="block truncate text-xs text-muted-foreground" data-user-content>{item.description}</span>{/if}
						</span>
						<span class="text-sm whitespace-nowrap text-muted-foreground">{item.members} {item.members === 1 ? 'person' : 'people'}</span>
						<span class="text-sm text-muted-foreground">{managesRoles && !item.fixed ? 'Edit' : 'View'}</span>
					</a>
				</li>
			{/each}
		</ul>
	{/if}
</SettingsFrame>

<Dialog.Root bind:open={making}>
	<Dialog.Content class="sm:max-w-md">
		<form class="grid gap-5" onsubmit={addRole} aria-label="New role">
			<Dialog.Header>
				<Dialog.Title>New role</Dialog.Title>
				<Dialog.Description>
					A role says what the people who hold it may do. It starts able to do nothing; you choose what it may do next{forInvite
						? ', then go back to your invitation'
						: ''}.
				</Dialog.Description>
			</Dialog.Header>
			<div class="grid gap-1.5 text-sm">
				<label class="font-medium" for="role-name">Name</label>
				<!-- svelte-ignore a11y_autofocus -->
				<Input id="role-name" bind:value={roleName} required maxlength={100} autofocus aria-describedby="role-name-hint" />
				<span id="role-name-hint" class="text-xs text-muted-foreground">For example Copy desk, Translator or Guest writer.</span>
			</div>
			<div class="grid gap-1.5 text-sm">
				<label class="font-medium" for="role-key">Short name</label>
				<Input
					id="role-key"
					class="font-mono"
					value={effectiveKey}
					oninput={(event) => {
						roleKeyTouched = true;
						roleKey = event.currentTarget.value;
					}}
					required
					maxlength={63}
					aria-describedby="role-key-hint"
				/>
				<span id="role-key-hint" class="text-xs text-muted-foreground">Made from the name: lowercase letters, digits and hyphens. Used by the API; it can’t change later.</span>
			</div>
			{#if roleError}<p role="alert" class="text-sm text-destructive" data-testid="role-error">{roleError}</p>{/if}
			<Dialog.Footer>
				<Button type="button" variant="outline" onclick={() => (making = false)}>Cancel</Button>
				<Button type="submit" disabled={roleBusy || roleName.trim() === '' || effectiveKey === ''}>{roleBusy ? 'Creating…' : 'Create and choose what it may do'}</Button>
			</Dialog.Footer>
		</form>
	</Dialog.Content>
</Dialog.Root>
