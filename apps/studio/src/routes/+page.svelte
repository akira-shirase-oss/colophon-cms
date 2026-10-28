<!--
  The Studio's front door (step 3.7, D-059): signed out, sign in or create an account; signed in
  on an installation nobody has set up yet, enter the setup code its server printed (D-061);
  then your projects. An administrator with none is offered their first; anyone else learns that
  projects are joined by invitation, and what to ask for.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import MailIcon from '@lucide/svelte/icons/mail';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, claimSetup, me, signInUrl, signUpUrl } from '#lib/api.ts';
	import { lastProject } from '#lib/lastProject.ts';
	import MyWork from '#lib/MyWork.svelte';
	import NewProject from '#lib/NewProject.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	let creating = $state(false);

	// The setup code (D-061): typed or pasted from the server's terminal.
	let code = $state('');
	let setupError = $state('');
	let setupBusy = $state(false);

	async function claim(event: SubmitEvent) {
		event.preventDefault();
		setupBusy = true;
		setupError = '';
		try {
			await claimSetup(code);
			code = '';
			await queryClient.invalidateQueries({ queryKey: ['me'] });
		} catch (failure) {
			setupError =
				failure instanceof ApiError && failure.reason === 'setup_code_invalid'
					? "That code isn't right, or it was used or has expired. Copy the code the server printed when it last started; restarting the server prints a new one."
					: failure instanceof ApiError
						? failure.message
						: String(failure);
		} finally {
			setupBusy = false;
		}
	}

	const bootstrapCommand = $derived(`colophon admin bootstrap --subject ${who.data?.subject ?? ''}`);

	// Whose work the home page shows: the project last opened in this browser, or the first. With
	// one project there is nothing to choose.
	let chosen = $state<string | null>(null);
	const projects = $derived(who.data?.projects ?? []);
	const remembered = $derived(who.data ? lastProject(who.data.id) : null);
	const current = $derived(
		projects.find((project) => project.id === chosen) ?? projects.find((project) => project.id === remembered) ?? projects[0]
	);
</script>

<svelte:head><title>{who.data && who.data.projects.length > 0 ? 'Your work' : 'Projects'} · Colophon Studio</title></svelte:head>

{#if who.isPending}
	<Skeleton class="mb-4 h-10 w-48" />
	<Skeleton class="h-32 w-full" />
{:else if who.isError}
	<Problem error={who.error} />
{:else if who.data === null}
	<div class="mx-auto max-w-md py-16 text-center">
		{#if page.url.searchParams.get('signed-out') === '1'}
			<p role="status" class="mb-8 rounded-xl border border-border bg-card px-4 py-3 text-sm text-pretty" data-testid="signed-out-note">
				You're signed out of Colophon. On a shared computer, close the browser too: the sign-in page may still remember you.
			</p>
		{/if}
		<h1 class="text-3xl font-semibold tracking-tight">Welcome to Colophon</h1>
		<p class="mt-3 text-muted-foreground" data-testid="signed-out">Write, review and publish your website's content, and see what reached your readers.</p>
		<div class="mt-8 flex flex-wrap justify-center gap-3">
			<Button href={signInUrl(resolve('/'))} data-sveltekit-reload data-testid="sign-in">Sign in<ArrowRightIcon data-icon="inline-end" /></Button>
			<Button variant="outline" href={signUpUrl(resolve('/'))} data-sveltekit-reload data-testid="create-account">Create an account</Button>
		</div>
		<p class="mt-6 text-sm text-muted-foreground">New here? Create an account, then ask the person who runs this Colophon to invite you to a project.</p>
	</div>
{:else if !who.data.installation.has_administrator}
	<div class="mx-auto max-w-xl py-8" data-testid="setup">
		<div class="mb-6 flex size-10 items-center justify-center rounded-lg border border-border bg-card shadow-xs">
			<KeyRoundIcon class="size-5 text-muted-foreground" aria-hidden="true" />
		</div>
		<h1 class="text-2xl font-semibold tracking-tight">Finish setting up Colophon</h1>
		<p class="mt-3 text-pretty text-muted-foreground">
			Nobody runs this Colophon yet. When its server started, it printed a <strong class="font-medium text-foreground">setup code</strong> in
			its terminal. Enter it here to become the administrator: you'll create projects and invite people.
		</p>
		<form class="mt-8 grid gap-2" onsubmit={claim} aria-label="Become the administrator">
			<Label for="setup-code">Setup code</Label>
			<div class="flex flex-col gap-2 sm:flex-row">
				<Input
					id="setup-code"
					class="font-mono tracking-wider uppercase"
					bind:value={code}
					required
					maxlength={64}
					autocomplete="off"
					spellcheck="false"
					placeholder="XXXXX-XXXXX-XXXXX-XXXXX"
				/>
				<Button type="submit" disabled={setupBusy || code.trim() === ''}>{setupBusy ? 'Checking…' : 'Become the administrator'}</Button>
			</div>
			<p class="text-sm text-muted-foreground">It looks like <span class="font-mono">7K3QD-0ZX1V-…</span>, works once, and expires an hour after the server started.</p>
			{#if setupError}<p role="alert" class="text-sm text-destructive" data-testid="setup-error">{setupError}</p>{/if}
		</form>
		<p class="mt-8 text-sm text-muted-foreground">
			Signed in as {who.data.display_name ?? who.data.subject}. Not the person who runs the server? Ask them to sign in here and enter the code; then
			they can invite you to a project.
		</p>
		<TechnicalDetails class="mt-6">
			<p>The server prints the code to standard error once, when it starts while nobody is an administrator. Instead of a code, someone with the database credentials can run:</p>
			<code class="block overflow-x-auto rounded bg-background px-2 py-1.5 font-mono">{bootstrapCommand}</code>
			<p>Your identity: <span class="font-mono">{who.data.subject}</span> at <span class="font-mono">{who.data.issuer}</span>.</p>
		</TechnicalDetails>
	</div>
{:else}
	<PageHeader title={who.data.projects.length > 0 ? 'Your work' : 'Projects'}>
		{#snippet meta()}
			<span data-testid="signed-in">Signed in as {who.data?.display_name ?? who.data?.id}{who.data?.administrator ? ', administrator' : ''}</span>
		{/snippet}
		{#snippet actions()}
			{#if who.data?.administrator && who.data.projects.length > 0}
				<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />New project</Button>
			{/if}
		{/snippet}
	</PageHeader>

	{#if who.data.projects.length === 0 && who.data.administrator}
		<section class="max-w-4xl" aria-labelledby="first-project" data-testid="first-project">
			<h2 id="first-project" class="mb-1 text-lg font-semibold">Create your first project</h2>
			<p class="mb-6 text-sm text-muted-foreground">A project is one website's content: its articles or pages, the people who write them, and what has gone live.</p>
			<NewProject />
		</section>
	{:else if who.data.projects.length === 0}
		<EmptyState icon={MailIcon} title="You're not in a project yet" testid="not-administrator">
			<p>
				Projects are joined by invitation link. Ask the person who runs this Colophon, or a project's owner, to invite you. Tell them the name you signed
				in with, <strong class="font-medium text-foreground">{who.data.display_name ?? who.data.subject}</strong>, and what you'll do: <em>Editor</em> to
				write, <em>Publisher</em> to approve and publish.
			</p>
			<p class="mt-2">When the link arrives, open it in this browser and press Join.</p>
		</EmptyState>
	{:else}
		{#if current}
			<div class="mb-4 flex flex-wrap items-center justify-between gap-3">
				<p class="text-sm text-muted-foreground">In <strong class="font-medium text-foreground" data-user-content data-testid="work-project">{current.name}</strong>, as {current.role_name}</p>
				{#if projects.length > 1}
					<label class="flex items-center gap-2 text-sm">
						<span class="text-muted-foreground">Show work in</span>
						<select
							class="h-9 max-w-56 rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
							value={current.id}
							onchange={(event) => (chosen = event.currentTarget.value)}
							data-testid="work-picker"
						>
							{#each projects as project (project.id)}<option value={project.id}>{project.name}</option>{/each}
						</select>
					</label>
				{/if}
			</div>
			{#key current.id}
				<MyWork project={current.id} identity={who.data.id} />
			{/key}
		{/if}
		{#if projects.length > 1}
			<h2 class="mt-12 mb-3 text-base font-semibold" id="all-projects">All projects</h2>
			<ul class="stagger grid gap-3 sm:grid-cols-2 lg:grid-cols-3" data-testid="projects" aria-labelledby="all-projects">
				{#each projects as project, index (project.id)}
					<li style="--i: {index}">
						<a
							href={resolve('/projects/[project]', { project: project.id })}
							class="lift group flex h-full items-center gap-3 rounded-xl border border-border bg-card p-3 shadow-xs outline-none hover:border-foreground/20 focus-visible:ring-[3px] focus-visible:ring-ring/40"
						>
							<span class="flex size-9 shrink-0 items-center justify-center rounded-lg bg-secondary text-sm font-semibold">{project.name.slice(0, 1).toUpperCase()}</span>
							<span class="min-w-0 flex-1" data-user-content>
								<span class="block truncate font-medium">{project.name}</span>
								<span class="block truncate text-xs text-muted-foreground">{project.role_name}</span>
							</span>
							<ArrowRightIcon class="size-4 shrink-0 text-muted-foreground transition-transform group-hover:translate-x-0.5" aria-hidden="true" />
						</a>
					</li>
				{/each}
			</ul>
		{/if}
		{#if !who.data.administrator}
			<p class="mt-8 text-sm text-muted-foreground" data-testid="not-administrator">To work on another project, ask its owner for an invitation link.</p>
		{/if}
	{/if}
{/if}

<Dialog.Root bind:open={creating}>
	<Dialog.Content class="max-h-[90svh] overflow-y-auto sm:max-w-3xl">
		<Dialog.Header>
			<Dialog.Title>New project</Dialog.Title>
			<Dialog.Description>A project is one website's content, its people and its releases.</Dialog.Description>
		</Dialog.Header>
		<NewProject />
	</Dialog.Content>
</Dialog.Root>

<p class="mt-16 text-sm text-muted-foreground">
	<a class="underline decoration-border underline-offset-4 hover:decoration-foreground" href={resolve('/playground')}>Editor playground</a>: every editing feature, on
	sample content, without saving.
</p>
