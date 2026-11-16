<!--
  Adding the website starter to a project (W.0): the dialog shows the plan — what a run creates
  and what is already there — then installs through the ordinary APIs and offers the seeded Home
  page. Runs and reruns are safe: whatever exists is kept.
-->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import { resolve } from '$app/paths';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { plainError } from '#lib/errors.ts';
	import { installSite, planSite, type SitePlan } from '#lib/siteTemplate.ts';

	let { project, open = $bindable(false), ondone }: { project: string; open?: boolean; ondone?: () => void } = $props();

	let plan = $state<SitePlan | null>(null);
	let planError = $state('');
	let steps = $state<string[]>([]);
	let busy = $state(false);
	let error = $state('');
	let home = $state('');

	function show(wants: boolean) {
		open = wants;
	}

	function load() {
		plan = null;
		planError = '';
		error = '';
		home = '';
		steps = [];
		planSite(project).then(
			(result) => (plan = result),
			(problem) => (planError = plainError(problem))
		);
	}

	$effect(() => {
		if (open) load();
	});

	async function install() {
		busy = true;
		error = '';
		try {
			home = (await installSite(project, (line) => (steps = [...steps, line]))).home;
			ondone?.();
		} catch (problem) {
			error = plainError(problem);
		} finally {
			busy = false;
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content>
		<Dialog.Header>
			<Dialog.Title>A website for this project</Dialog.Title>
			<Dialog.Description>
				Pages with a section library, a navigation menu and a theme with the whole design system — added as ordinary content types and
				entries you can change after.
			</Dialog.Description>
		</Dialog.Header>
		{#if planError !== ''}
			<p role="alert" class="rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive">{planError}</p>
			<Dialog.Footer>
				<Button variant="outline" onclick={() => show(false)}>Close</Button>
				<Button onclick={load}>Try again</Button>
			</Dialog.Footer>
		{:else if home !== ''}
			<ul class="grid gap-1.5 py-1 text-sm" data-testid="site-install-steps">
				{#each steps as step (step)}
					<li class="flex items-center gap-2"><CheckIcon class="size-4 text-live" aria-hidden="true" />{step}</li>
				{/each}
			</ul>
			<p class="text-sm text-muted-foreground">The Home page has two starter sections; the theme holds the default palette.</p>
			<Dialog.Footer>
				<Button variant="outline" onclick={() => show(false)}>Done</Button>
				<Button
					variant="default"
					onclick={() => show(false)}
					href={resolve('/projects/[project]/[collection]/[entry]', { project, collection: 'pages', entry: home })}
					data-testid="site-open-home"
				>
					Open the Home page
				</Button>
			</Dialog.Footer>
		{:else if plan === null}
			<div class="grid gap-2 py-2"><Skeleton class="h-5 w-3/4" /><Skeleton class="h-5 w-1/2" /></div>
		{:else}
			{#if error !== ''}<p role="alert" class="mb-2 rounded-lg bg-destructive/10 px-4 py-3 text-sm text-destructive">{error}</p>{/if}
			{#if busy}
				<ul class="grid gap-1.5 py-1 text-sm" data-testid="site-install-steps">
					{#each steps as step (step)}
						<li class="flex items-center gap-2"><CheckIcon class="size-4 text-live" aria-hidden="true" />{step}</li>
					{/each}
				</ul>
			{:else if plan.done}
				<p class="py-1 text-sm" data-testid="site-installed">Everything the website starter adds is already in this project.</p>
			{:else}
				<div class="grid gap-3 py-1 text-sm" data-testid="site-plan">
					{#if plan.creates.length > 0}
						<div>
							<p class="mb-1.5 font-medium">This adds</p>
							<ul class="grid gap-1">
								{#each plan.creates as line (line)}
									<li class="flex items-center gap-2"><PlusIcon class="size-4 text-muted-foreground" aria-hidden="true" />{line}</li>
								{/each}
							</ul>
						</div>
					{/if}
					{#if plan.keeps.length > 0}
						<div>
							<p class="mb-1.5 font-medium text-muted-foreground">Already here, kept as it is</p>
							<ul class="grid gap-1 text-muted-foreground">
								{#each plan.keeps as line (line)}
									<li class="flex items-center gap-2"><CheckIcon class="size-4" aria-hidden="true" />{line}</li>
								{/each}
							</ul>
						</div>
					{/if}
				</div>
			{/if}
			<Dialog.Footer>
				<Button variant="outline" disabled={busy} onclick={() => show(false)}>Cancel</Button>
				{#if !plan.done}<Button onclick={install} disabled={busy} data-testid="site-install">{busy ? 'Adding…' : 'Add to this project'}</Button>{/if}
			</Dialog.Footer>
		{/if}
	</Dialog.Content>
</Dialog.Root>
