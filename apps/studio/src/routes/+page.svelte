<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import FolderIcon from '@lucide/svelte/icons/folder';
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, createProject, me, signInUrl } from '#lib/api.ts';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import FormRow from '#lib/shell/FormRow.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';

	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));

	let name = $state('');
	let slug = $state('');
	// The slug follows the name until someone edits it.
	let slugEdited = $state(false);
	let error = $state('');
	let busy = $state(false);

	const slugOf = (text: string) =>
		text
			.toLowerCase()
			.replace(/[^a-z0-9]+/g, '-')
			.replace(/^-+|-+$/g, '')
			.slice(0, 63)
			.replace(/-+$/, '');

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			const project = await createProject(slug, name);
			await queryClient.invalidateQueries({ queryKey: ['me'] });
			await goto(resolve('/projects/[project]', { project: project.id }));
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
		} finally {
			busy = false;
		}
	}

	const ROLE_WORD = { owner: 'Owner', developer: 'Developer', editor: 'Editor', publisher: 'Publisher' } as const;
</script>

<svelte:head><title>Projects · Colophon Studio</title></svelte:head>

{#if who.isPending}
	<Skeleton class="mb-4 h-10 w-48" />
	<Skeleton class="h-32 w-full" />
{:else if who.isError}
	<Problem error={who.error} />
{:else if who.data === null}
	<div class="mx-auto max-w-md py-16 text-center">
		<h1 class="font-serif text-4xl font-medium tracking-tight">Welcome to Colophon</h1>
		<p class="mt-3 text-muted-foreground" data-testid="signed-out">Sign in to write and edit.</p>
		<Button class="mt-8" href={signInUrl(resolve('/'))} data-sveltekit-reload data-testid="sign-in">Sign in<ArrowRightIcon data-icon="inline-end" /></Button>
	</div>
{:else}
	<PageHeader title="Projects">
		{#snippet meta()}
			<span data-testid="signed-in">Signed in as {who.data?.display_name ?? who.data?.id}{who.data?.administrator ? ', administrator' : ''}</span>
		{/snippet}
	</PageHeader>

	{#if who.data.projects.length === 0}
		<EmptyState icon={FolderIcon} title="No projects yet">
			{who.data.administrator ? 'Create the first one below.' : 'Ask a project owner for an invitation link.'}
		</EmptyState>
	{:else}
		<ul class="grid gap-3 sm:grid-cols-2 lg:grid-cols-3" data-testid="projects">
			{#each who.data.projects as project (project.id)}
				<li>
					<a
						href={resolve('/projects/[project]', { project: project.id })}
						class="group flex h-full flex-col gap-3 rounded-xl border border-border bg-card p-4 shadow-xs transition-colors outline-none hover:border-foreground/20 focus-visible:ring-[3px] focus-visible:ring-ring/40"
					>
						<span class="flex size-9 items-center justify-center rounded-lg bg-secondary font-serif text-lg">{project.name.slice(0, 1).toUpperCase()}</span>
						<span>
							<span class="block font-medium">{project.name}</span>
							<span class="font-mono text-xs text-muted-foreground">{project.slug}</span>
						</span>
						<span class="mt-auto flex items-center justify-between text-sm text-muted-foreground">
							{ROLE_WORD[project.role]}
							<ArrowRightIcon class="size-4 transition-transform group-hover:translate-x-0.5" aria-hidden="true" />
						</span>
					</a>
				</li>
			{/each}
		</ul>
	{/if}

	{#if who.data.administrator}
		<section class="mt-12 max-w-3xl" aria-labelledby="new-project">
			<h2 id="new-project" class="mb-1 text-base font-semibold">New project</h2>
			<p class="mb-6 text-sm text-muted-foreground">A project holds its content, members, releases and where they go.</p>
			<form onsubmit={create} aria-label="New project">
				<FormRow label="Name" hint="What people call it." for="project-name">
					<Input
						id="project-name"
						bind:value={name}
						required
						maxlength={200}
						oninput={() => {
							if (!slugEdited) slug = slugOf(name);
						}}
					/>
				</FormRow>
				<FormRow label="Slug (in addresses and the CLI)" hint="Lowercase letters, digits and dashes." for="project-slug">
					<Input
						id="project-slug"
						class="font-mono"
						bind:value={slug}
						required
						maxlength={63}
						pattern="[a-z0-9]([a-z0-9\-]*[a-z0-9])?"
						oninput={() => (slugEdited = true)}
					/>
				</FormRow>
				{#if error}<p role="alert" class="mb-4 text-sm text-destructive">{error}</p>{/if}
				<div class="flex justify-end pt-2"><Button type="submit" disabled={busy}>Create project</Button></div>
			</form>
		</section>
	{:else}
		<p class="mt-8 text-sm text-muted-foreground" data-testid="not-administrator">
			Only an administrator can create projects. To work on one, ask its owner for an invitation link.
		</p>
	{/if}
{/if}
<p class="mt-16 text-sm text-muted-foreground">
	<a class="underline decoration-border underline-offset-4 hover:decoration-foreground" href={resolve('/playground')}>Editor playground</a>: every editing feature, on the
	shared fixtures, without saving.
</p>
