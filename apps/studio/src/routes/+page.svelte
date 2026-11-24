<!--
  The Studio's front door (step 3.7, D-059): signed out, sign in or create an account; signed in
  on an installation nobody has set up yet, enter the setup code its server printed (D-061);
  then your projects. An administrator with none is offered their first; anyone else learns that
  projects are joined by invitation, and what to ask for. On an installation with no administrator
  yet (`GET /api/v1/installation`), or opened from the server's setup link (`#setup=<code>`), it
  says this Colophon is new and makes Create an account the first choice (D-147); the link's code
  is filled in after signing in.
-->
<script lang="ts">
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import MailIcon from '@lucide/svelte/icons/mail';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { goto, replaceState } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { page } from '$app/state';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError, claimSetup, installation, me } from '#lib/api.ts';
	import { providersQuery } from '#lib/queries.ts';
	import { forgetSetupCode, keepSetupCode, keptSetupCode, setupCodeFrom } from '#lib/setupLink.ts';
	import NewProject from '#lib/NewProject.svelte';
	import AuthForm from '#lib/shell/AuthForm.svelte';
	import EmptyState from '#lib/shell/EmptyState.svelte';
	import PageHeader from '#lib/shell/PageHeader.svelte';
	import Problem from '#lib/shell/Problem.svelte';
	import TechnicalDetails from '#lib/shell/TechnicalDetails.svelte';

	const queryClient = useQueryClient();
	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	// Signed out, whether anyone runs this Colophon yet: nobody does on a fresh installation. Until
	// someone does, also how it was installed: a container's setup code is in its compose logs.
	const install = createQuery(() => ({
		queryKey: ['installation'],
		queryFn: installation,
		enabled: who.data === null || who.data?.installation.has_administrator === false
	}));
	const inContainer = $derived(install.data?.install_kind === 'container');
	const logsCommand = 'docker compose logs colophon | grep "Setup code"';
	// Whether a provider exists — the signed-out note's "close the browser" advice only makes
	// sense when sign-in could live somewhere else too (D-146).
	const providers = createQuery(providersQuery);
	// Who may create projects (D-235): an administrator always; anyone else when registration is open
	// (self-serve). The server still holds each account to its project cap (409 project_limit).
	const canCreate = $derived(who.data?.administrator === true || providers.data?.registration === 'open');
	let creating = $state(false);

	// The setup code (D-061): typed or pasted from the server's terminal, or filled in from the setup
	// link the server printed beside it (D-147). The link's code is kept in this tab through signing
	// in, and out of the address bar, so it isn't left in the history or a bookmark.
	// Read whenever the address's fragment changes, since a link pasted into a tab already on this
	// page changes only the fragment and doesn't open the page afresh.
	const keptAtStart = keptSetupCode();
	let kept = $state(keptAtStart);
	let code = $state(keptAtStart ?? '');
	$effect(() => {
		const linked = setupCodeFrom(page.url.hash);
		if (linked === null) return;
		keepSetupCode(linked);
		kept = linked;
		code = linked;
		replaceState(`${page.url.pathname}${page.url.search}`, {});
	});
	const fromLink = $derived(kept !== null && code.trim().toUpperCase() === kept);
	let setupError = $state('');
	let setupBusy = $state(false);
	function forget() {
		forgetSetupCode();
		kept = null;
	}
	// Once anyone is an administrator, a kept code is dead: forget it.
	$effect(() => {
		if (who.data?.installation.has_administrator === true && kept !== null) forget();
	});

	async function claim(event: SubmitEvent) {
		event.preventDefault();
		setupBusy = true;
		setupError = '';
		try {
			await claimSetup(code);
			code = '';
			forget();
			await queryClient.invalidateQueries({ queryKey: ['me'] });
		} catch (failure) {
			// The link's own code refused (used, expired, or from an older start): it won't work later either.
			if (failure instanceof ApiError && failure.reason === 'setup_code_invalid' && fromLink) forget();
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

	const projects = $derived(who.data?.projects ?? []);
	// One project: straight into it. Several: the list. `?all` (the switcher's "All projects")
	// keeps the list even with one, so a one-project account can still make another.
	$effect(() => {
		const only = who.data?.projects.length === 1 ? who.data.projects[0] : null;
		if (only && !page.url.searchParams.has('all')) {
			void goto(resolve('/projects/[project]', { project: only.id }), { replaceState: true });
		}
	});

	// "All projects" grows without bound on a busy installation, so it can be filtered by name —
	// and even unfiltered it is capped: hundreds of rows hide the project someone wants, not show it.
	const SHOWN = 24;
	let filter = $state('');
	const matching = $derived(
		filter.trim() === ''
			? projects
			: projects.filter((project) => project.name.toLowerCase().includes(filter.trim().toLowerCase()))
	);
	const shown = $derived(matching.slice(0, SHOWN));
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
				You're signed out of Colophon.{#if providers.data?.sso !== false} The sign-in page may still remember you, so on a shared computer, close the browser.{/if}
			</p>
		{/if}
		<h1 class="page-title">Welcome to Colophon</h1>
		{#if kept !== null || install.data?.has_administrator === false}
			<!-- Nobody is an administrator yet, or opened from the setup link the server prints only then:
				the card opens on creating an account, the first visitor's one job (D-147). -->
			<div data-testid="new-installation">
				<p class="mt-3 text-pretty text-muted-foreground" data-testid="signed-out">
					This Colophon is new. Create the first account; you'll become its administrator with the setup code the server printed.
				</p>
				<div class="mt-8 text-left">
					<AuthForm start="signup" firstRun returnTo={page.url.searchParams.get('return_to') ?? resolve('/')} navigateAfter={page.url.searchParams.has('return_to')} />
				</div>
				<p class="mt-6 text-sm text-pretty text-muted-foreground">
					{#if kept !== null}
						Your link carries the setup code, so you won't need to type it. Already have an account here? Sign in instead.
					{:else if inContainer}
						The server printed the setup code, and a link that fills it in, to its log when it started. Already have an account here? Sign in instead.
					{:else}
						The server printed the setup code, and a link that fills it in, when it started. Already have an account here? Sign in instead.
					{/if}
				</p>
				{#if kept === null && inContainer}
					<div class="mt-4 text-left text-sm" data-testid="setup-code-from-logs">
						<p class="text-muted-foreground">To see it, run this in the folder you started Colophon from:</p>
						<code class="mt-1.5 block overflow-x-auto rounded-lg border border-border bg-card px-3 py-2 font-mono">{logsCommand}</code>
					</div>
				{/if}
			</div>
		{:else}
			<p class="mt-3 text-muted-foreground" data-testid="signed-out">Write, review and publish your website's content, and see what reached your readers.</p>
			<div class="mt-8 text-left">
				<AuthForm returnTo={page.url.searchParams.get('return_to') ?? resolve('/')} navigateAfter={page.url.searchParams.has('return_to')} />
			</div>
			<p class="mt-6 text-sm text-muted-foreground">
				{#if providers.data?.registration === 'open'}
					New here? Create an account and start your first project.
				{:else if providers.data?.registration === 'closed'}
					Need an account? Ask the person who runs this Colophon.
				{:else}
					New here? Open the invitation link you were sent to create your account.
				{/if}
			</p>
		{/if}
	</div>
{:else if !who.data.installation.has_administrator}
	<div class="mx-auto max-w-xl py-8" data-testid="setup">
		<div class="mb-6 flex size-10 items-center justify-center rounded-lg border border-border bg-card shadow-xs">
			<KeyRoundIcon class="size-5 text-muted-foreground" aria-hidden="true" />
		</div>
		<h1 class="page-title">Finish setting up Colophon</h1>
		<p class="mt-3 text-pretty text-muted-foreground">
			Nobody runs this Colophon yet. When its server started, it printed a <strong class="font-medium text-foreground">setup code</strong> in
			its {inContainer ? 'log' : 'terminal'}. Enter it here to become the administrator: you'll create projects and invite people.
		</p>
		{#if inContainer}
			<div class="mt-4 text-sm" data-testid="setup-code-from-logs">
				<p class="text-muted-foreground">To see it, run this in the folder you started Colophon from:</p>
				<code class="mt-1.5 block overflow-x-auto rounded-lg border border-border bg-card px-3 py-2 font-mono">{logsCommand}</code>
			</div>
		{/if}
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
			{#if fromLink}
				<p class="flex items-center gap-1.5 text-sm font-medium" role="status" data-testid="setup-from-link">
					<CircleCheckIcon class="size-4 text-live" aria-hidden="true" />Code filled in from your link.
				</p>
			{/if}
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
	<PageHeader title="Projects">
		{#snippet meta()}
			<span data-testid="signed-in">Signed in as {who.data?.display_name ?? who.data?.id}{who.data?.administrator ? ', administrator' : ''}</span>
		{/snippet}
		{#snippet actions()}
			{#if canCreate && (who.data?.projects.length ?? 0) > 0}
				<Button onclick={() => (creating = true)}><PlusIcon data-icon="inline-start" />New project</Button>
			{/if}
		{/snippet}
	</PageHeader>

	{#if who.data.projects.length === 0 && canCreate}
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
		{#if projects.length > 0}
			<div class="mb-3 flex flex-wrap items-center justify-between gap-3">
				<h2 class="text-base font-semibold" id="all-projects">Your projects</h2>
				<Input
					class="h-9 w-full max-w-64"
					placeholder="Search projects"
					aria-label="Search projects"
					bind:value={filter}
					data-testid="project-search"
				/>
			</div>
			{#if matching.length === 0}
				<p class="text-sm text-muted-foreground" data-testid="projects-no-match">
					No project matches <strong class="font-medium text-foreground">{filter.trim()}</strong>.
				</p>
			{:else}
				<ul class="stagger grid gap-3 sm:grid-cols-2 lg:grid-cols-3 2xl:grid-cols-4" data-testid="projects" aria-labelledby="all-projects">
					{#each shown as project, index (project.id)}
						<li style="--i: {index}">
							<a
								href={resolve('/projects/[project]', { project: project.id })}
								class="lift group flex h-full items-center gap-3 rounded-xl border border-border bg-card p-3 outline-none hover:border-foreground/20 focus-visible:ring-[3px] focus-visible:ring-ring/40"
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
				{#if matching.length > shown.length}
					<p class="mt-3 text-sm text-muted-foreground" data-testid="projects-capped">
						Showing {shown.length} of {matching.length} — search by name to find the rest.
					</p>
				{/if}
			{/if}
		{/if}
		{#if !canCreate}
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

<p class={['mt-16 text-sm text-muted-foreground', who.data === null && 'text-center']}>
	<a class="underline decoration-border underline-offset-4 hover:decoration-foreground" href={resolve('/playground')}>Editor playground</a>: every editing feature, on
	sample content, without saving.
</p>
