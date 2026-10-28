<!--
  What a role may do (D-058, step R.2), as a grid in plain words: a row for everything and one per
  content type, a column per kind of work, and below it the powers over the whole project. A tick
  in "Everything" covers every content type, including ones added later. Saving names the version
  edited, so two people changing one role can't overwrite each other unseen. The owner role is
  shown, never edited; nobody can give a role more than their own role holds, and the server says
  so when someone tries.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Textarea } from '@colophon/ui/ui/textarea';
	import { type Action, ApiError, type ContentType, deleteRole, type Grant, type RoleDefinition, updateRole } from '#lib/api.ts';
	import PermissionGrid from '#lib/PermissionGrid.svelte';
	import { cell } from '#lib/permissions.ts';
	import FormRow from '#lib/shell/FormRow.svelte';

	let { project, role, types, editable }: { project: string; role: RoleDefinition; types: ContentType[]; editable: boolean } = $props();

	const queryClient = useQueryClient();

	const key = cell;
	// The editor's copy, keyed on the role's version by the page: a save starts a fresh copy.
	// svelte-ignore state_referenced_locally
	const saved = role;
	// The grid edits plain permissions; ones limited to some fields or to a member's own entries
	// (step R.3) are kept apart, listed in words, and saved exactly as they are.
	const limited = (grant: Grant) => grant.fields !== undefined || grant.condition !== undefined;
	const initial = saved.permissions.filter((grant) => !limited(grant)).map((grant) => key(grant.action, grant.subject));
	const initialLimits = JSON.stringify(saved.permissions.filter(limited));
	let name = $state(saved.name);
	let description = $state(saved.description);
	let grants = $state(new Set<string>(initial));
	let limits = $state<Grant[]>(saved.permissions.filter(limited));
	let error = $state('');
	let stale = $state(false);
	let busy = $state(false);
	let confirming = $state(false);

	const dirty = $derived(
		name !== saved.name ||
			description !== saved.description ||
			grants.size !== initial.length ||
			initial.some((grant) => !grants.has(grant)) ||
			JSON.stringify(limits) !== initialLimits
	);
	const used = $derived(role.members + role.invitations);

	function permissions(): Grant[] {
		const plain = [...grants].map((grant) => {
			const [action, subject] = grant.split('|') as [Action, string];
			return { action, subject };
		});
		return [...plain, ...limits];
	}

	/** Work on content that can be limited, in plain words: fields for seeing, creating and
	 *  editing; a member's own entries for seeing, editing and deleting. */
	const LIMITABLE: { action: Action; label: string; fields: boolean; own: boolean }[] = [
		{ action: 'content.read', label: 'See', fields: true, own: true },
		{ action: 'content.create', label: 'Create', fields: true, own: false },
		{ action: 'content.update', label: 'Edit', fields: true, own: true }
	];
	let limitAction = $state<Action>('content.update');
	let limitSubject = $state('*');
	let limitOwn = $state(true);
	let limitFields = $state<string[]>([]);
	const limitKind = $derived(LIMITABLE.find((kind) => kind.action === limitAction));
	const limitTypeFields = $derived(
		limitSubject === '*'
			? [...new Set(types.flatMap((type) => type.definition.fields.map((field) => field.id)))]
			: (types.find((type) => type.slug === limitSubject)?.definition.fields.map((field) => field.id) ?? [])
	);

	function describe(grant: Grant): string {
		const what = LIMITABLE.find((kind) => kind.action === grant.action)?.label ?? grant.action;
		const where = grant.subject === '*' ? 'every content type' : (types.find((type) => type.slug === grant.subject)?.definition.name ?? grant.subject);
		const fields = grant.fields ? `, only ${grant.fields.join(', ')}` : '';
		const own = grant.condition === 'own' ? ', only their own entries' : '';
		return `${what}: ${where}${fields}${own}`;
	}

	function addLimit() {
		const own = limitOwn && (limitKind?.own ?? false);
		const fields = limitFields.length > 0 ? [...limitFields].sort() : undefined;
		if (!own && fields === undefined) return;
		limits = [...limits, { action: limitAction, subject: limitSubject, ...(fields ? { fields } : {}), ...(own ? { condition: 'own' as const } : {}) }];
		limitFields = [];
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		stale = false;
		try {
			const saved = await updateRole(project, role.key, role.version, { name: name.trim(), description: description.trim(), permissions: permissions() });
			toast.success(`Saved ${saved.name}`);
			await Promise.all([
				queryClient.invalidateQueries({ queryKey: ['roles', project] }),
				queryClient.invalidateQueries({ queryKey: ['project', project] })
			]);
		} catch (failure) {
			stale = failure instanceof ApiError && failure.failure === 'conflict';
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}

	async function reload() {
		await queryClient.invalidateQueries({ queryKey: ['roles', project] });
	}

	async function remove() {
		busy = true;
		error = '';
		try {
			await deleteRole(project, role.key, role.version);
			toast.success(`Deleted ${role.name}`);
			await queryClient.invalidateQueries({ queryKey: ['roles', project] });
			await goto(resolve('/projects/[project]/settings/roles', { project }));
		} catch (failure) {
			stale = failure instanceof ApiError && failure.failure === 'conflict' && failure.reason === 'stale_role';
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
			confirming = false;
		}
	}
</script>

<form class="max-w-4xl" onsubmit={save} aria-label="Role {role.name}">
	{#if role.fixed}
		<p class="mb-6 rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="role-fixed">
			Owners can always do everything, so this role can't be changed. A project always keeps at least one owner.
		</p>
	{:else if !editable}
		<p class="mb-6 rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="role-read-only">
			Your role can't change roles. This is what {role.name} may do.
		</p>
	{/if}

	<FormRow label="Name" for="role-edit-name">
		<Input id="role-edit-name" class="max-w-md" bind:value={name} required maxlength={100} disabled={!editable} />
	</FormRow>
	<FormRow label="What it's for" hint="Shown when someone is invited with this role." for="role-edit-description">
		<Textarea id="role-edit-description" class="max-w-md" bind:value={description} maxlength={500} rows={2} disabled={!editable} />
	</FormRow>

	<PermissionGrid {types} bind:grants {editable}>
		{#snippet between()}
	<h2 class="mt-8 mb-1 text-base font-semibold" id="limited-heading">Limited permissions</h2>
	<p class="mb-4 text-sm text-muted-foreground">
		Narrower than the grid: only some fields, or only the entries a member created themselves. For example, edit the titles of any article and
		every field of their own.
	</p>
	<ul class="mb-4 overflow-hidden rounded-xl border border-border bg-card" aria-labelledby="limited-heading" data-testid="limited-permissions">
		{#each limits as grant, index (`${grant.action}|${grant.subject}|${grant.condition ?? ''}|${index}`)}
			<li class="rule-dotted flex items-center gap-3 px-4 py-3 text-sm last:border-b-0">
				<span class="min-w-0 flex-1">{describe(grant)}</span>
				{#if editable}
					<Button variant="ghost" size="sm" onclick={() => (limits = limits.filter((_, at) => at !== index))} aria-label="Remove {describe(grant)}">Remove</Button>
				{/if}
			</li>
		{:else}
			<li class="px-4 py-3 text-sm text-muted-foreground">None.</li>
		{/each}
	</ul>
	{#if editable}
		<fieldset class="mb-2 grid gap-3 rounded-xl border border-dashed border-border p-4" aria-label="Add a limited permission">
			<div class="flex flex-wrap gap-3">
				<label class="grid gap-1 text-sm">
					What
					<select class="h-9 rounded-md border border-input bg-card px-2 text-sm" bind:value={limitAction}>
						{#each LIMITABLE as kind (kind.action)}<option value={kind.action}>{kind.label}</option>{/each}
					</select>
				</label>
				<label class="grid gap-1 text-sm">
					Where
					<select class="h-9 rounded-md border border-input bg-card px-2 text-sm" bind:value={limitSubject} onchange={() => (limitFields = [])}>
						<option value="*">Every content type</option>
						{#each types as type (type.slug)}<option value={type.slug}>{type.definition.name}</option>{/each}
					</select>
				</label>
				{#if limitKind?.own}
					<label class="flex items-end gap-2 pb-2 text-sm">
						<input type="checkbox" class="size-4 accent-primary" bind:checked={limitOwn} />
						Only their own entries
					</label>
				{/if}
			</div>
			<div class="flex flex-wrap gap-x-4 gap-y-2 text-sm" role="group" aria-label="Only these fields (none ticked means every field)">
				{#each limitTypeFields as field (field)}
					<label class="flex items-center gap-1.5">
						<input type="checkbox" class="size-4 accent-primary" value={field} bind:group={limitFields} />
						<code class="font-mono text-xs">{field}</code>
					</label>
				{/each}
			</div>
			<div>
				<Button variant="secondary" size="sm" onclick={addLimit} disabled={!(limitOwn && limitKind?.own) && limitFields.length === 0}>Add this limit</Button>
			</div>
		</fieldset>
	{/if}

		{/snippet}
	</PermissionGrid>

	{#if error}
		<div role="alert" class="mt-6 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive" data-testid="role-error">
			<p>{stale ? 'Someone changed this role while you were editing it. Load their version, then make your change again.' : error}</p>
			{#if stale}<Button class="mt-2" variant="secondary" size="sm" onclick={reload}>Load the latest version</Button>{/if}
		</div>
	{/if}

	{#if editable}
		<div class="mt-6 flex flex-wrap items-center justify-between gap-3">
			<div class="text-sm text-muted-foreground">
				{#if role.built_in}Built in: you can change it, but not delete it while anyone holds it.{/if}
			</div>
			<Button type="submit" disabled={busy || !dirty || name.trim() === ''}>Save role</Button>
		</div>

		<section class="mt-12 border-t border-border pt-6" aria-labelledby="delete-role">
			<h2 id="delete-role" class="mb-1 text-base font-semibold">Delete this role</h2>
			{#if used > 0}
				<p class="text-sm text-muted-foreground" data-testid="role-in-use">
					{role.members}
					{role.members === 1 ? 'person holds' : 'people hold'} this role{role.invitations > 0
						? ` and ${role.invitations} pending invitation${role.invitations === 1 ? '' : 's'} offer${role.invitations === 1 ? 's' : ''} it`
						: ''}. Give them another role, or revoke the invitations, before deleting it.
				</p>
			{:else if confirming}
				<p class="mb-3 text-sm">Delete {role.name} for good? Nobody holds it, so nothing else changes.</p>
				<div class="flex gap-2">
					<Button variant="destructive" disabled={busy} onclick={remove}>Delete {role.name}</Button>
					<Button variant="ghost" disabled={busy} onclick={() => (confirming = false)}>Keep it</Button>
				</div>
			{:else}
				<p class="mb-3 text-sm text-muted-foreground">Nobody holds this role.</p>
				<Button variant="secondary" disabled={busy} onclick={() => (confirming = true)}>Delete role…</Button>
			{/if}
		</section>
	{/if}
</form>
