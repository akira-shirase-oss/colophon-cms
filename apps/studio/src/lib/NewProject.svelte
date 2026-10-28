<!--
  Making a project (administrators only, D-043): a name, its URL name (following the name until
  edited), and a starting point in plain words (step 3.7). The project is made first, then its
  starting point through the content-type API; if that second part fails, the project still
  exists and the message says what to do.
-->
<script lang="ts">
	import { goto } from '$app/navigation';
	import { resolve } from '$app/paths';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { ApiError, createProject } from './api.ts';
	import { slugify } from './content.ts';
	import FormRow from './shell/FormRow.svelte';
	import { applyStarter, type Starter, STARTERS } from './starters.ts';

	let { submitLabel = 'Create project' }: { submitLabel?: string } = $props();

	const queryClient = useQueryClient();
	const id = $props.id();
	let name = $state('');
	let urlName = $state('');
	// The URL name follows the name until someone edits it.
	let urlNameEdited = $state(false);
	let starter = $state<Starter>('blog');
	let error = $state('');
	let busy = $state(false);
	const suggested = $derived(slugify(name).slice(0, 63).replace(/-+$/, ''));
	const effective = $derived(urlNameEdited ? urlName : suggested);

	async function create(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		let project: string;
		try {
			project = (await createProject(effective, name.trim())).id;
		} catch (failure) {
			error = failure instanceof ApiError ? failure.message : String(failure);
			busy = false;
			return;
		}
		try {
			await applyStarter(project, starter);
		} catch (failure) {
			const why = failure instanceof ApiError ? failure.message : String(failure);
			toast.error(`The project was made, but its starting point didn't finish: ${why}. Content types, in the menu, can finish it.`);
		}
		await queryClient.invalidateQueries({ queryKey: ['me'] });
		busy = false;
		await goto(resolve('/projects/[project]', { project }));
	}
</script>

<form onsubmit={create} aria-label="New project" data-testid="new-project">
	<FormRow label="Name" hint="What people call it." for="project-name">
		<Input id="project-name" bind:value={name} required maxlength={200} placeholder="Field notes" />
	</FormRow>
	<FormRow label="URL name" hint="Used in addresses and the command line: lowercase letters, digits and dashes." for="project-url-name">
		<Input
			id="project-url-name"
			class="font-mono"
			value={effective}
			oninput={(event) => {
				urlNameEdited = true;
				urlName = event.currentTarget.value;
			}}
			required
			maxlength={63}
			pattern="[a-z0-9]([a-z0-9\-]*[a-z0-9])?"
		/>
	</FormRow>
	<fieldset class="rule-dotted grid gap-x-10 gap-y-3 py-5 sm:grid-cols-[minmax(0,13rem)_minmax(0,1fr)]">
		<legend class="sr-only">Start with</legend>
		<div class="space-y-0.5" aria-hidden="true">
			<p class="text-sm font-medium">Start with</p>
			<p class="text-xs text-muted-foreground">You can add and change kinds of content later.</p>
		</div>
		<div class="grid gap-2 sm:grid-cols-3">
			{#each STARTERS as option (option.id)}
				<label
					class="flex cursor-pointer flex-col gap-1 rounded-xl border border-border bg-card p-3.5 text-sm shadow-xs transition-colors hover:border-foreground/30 has-checked:border-foreground has-checked:ring-[3px] has-checked:ring-ring/30 has-focus-visible:ring-[3px] has-focus-visible:ring-ring/40"
				>
					<span class="flex items-center gap-2">
						<input
							type="radio"
							name="starter"
							class="size-4 accent-primary"
							value={option.id}
							bind:group={starter}
							aria-label={option.name}
							aria-describedby="{id}-{option.id}"
						/>
						<span class="font-medium" aria-hidden="true">{option.name}</span>
					</span>
					<span id="{id}-{option.id}" class="grid gap-1">
						<span class="text-muted-foreground">{option.description}</span>
						<span class="text-xs text-muted-foreground">{option.includes}</span>
					</span>
				</label>
			{/each}
		</div>
	</fieldset>
	{#if error}<p role="alert" class="mb-4 text-sm text-destructive">{error}</p>{/if}
	<div class="flex justify-end pt-2">
		<Button type="submit" disabled={busy || name.trim() === '' || effective === ''}>{busy ? 'Creating…' : submitLabel}</Button>
	</div>
</form>
