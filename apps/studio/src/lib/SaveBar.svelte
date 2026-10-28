<!--
  The builder's one save bar: it appears, stuck to the bottom of the screen, as soon as there are
  unsaved changes ("You have unsaved changes · Discard · Save"), says what must be fixed before
  saving, and stays while a save is on its way. Leaving for another page of the Studio with
  changes unsaved asks first, since they can't be brought back.
-->
<script lang="ts">
	import { beforeNavigate, goto } from '$app/navigation';
	import * as AlertDialog from '@colophon/ui/ui/alert-dialog';
	import { Button } from '@colophon/ui/ui/button';

	let {
		changed,
		busy,
		problems = [],
		what,
		onsave,
		ondiscard
	}: {
		changed: boolean;
		busy: boolean;
		/** What keeps a save from going, one sentence each. */
		problems?: string[];
		/** What is being changed, for the question before leaving: "Articles". */
		what: string;
		onsave: () => void;
		ondiscard: () => void;
	} = $props();

	let leaving = $state<URL | null>(null);
	let letGo = false;
	beforeNavigate((navigation) => {
		// Closing the tab or typing an address is the browser's to ask about; within the Studio, ask here.
		if (!changed || letGo || navigation.type === 'leave' || navigation.to === null) return;
		navigation.cancel();
		leaving = navigation.to.url;
	});
	async function leave() {
		const to = leaving;
		leaving = null;
		if (to === null) return;
		letGo = true;
		await goto(to);
		letGo = false;
	}
</script>

{#if changed || busy}
	<div
		class="sticky bottom-4 z-20 mt-6 flex flex-wrap items-center gap-x-4 gap-y-2 rounded-2xl border border-border bg-card px-4 py-3 shadow-float"
		role="region"
		aria-label="Unsaved changes"
		data-testid="save-bar"
	>
		<span class="size-2 shrink-0 rounded-full bg-pending" aria-hidden="true"></span>
		<p class="text-sm font-medium">{busy ? 'Saving…' : 'You have unsaved changes'}</p>
		<span class="flex-1"></span>
		<Button variant="ghost" disabled={busy} onclick={ondiscard}>Discard</Button>
		<Button disabled={busy || problems.length > 0} onclick={onsave} data-testid="save">Save</Button>
		{#if problems.length > 0}
			<ul class="w-full space-y-0.5 text-sm text-destructive" data-testid="builder-problems" aria-label="Before saving">
				{#each problems as problem (problem)}<li>{problem}</li>{/each}
			</ul>
		{/if}
	</div>
{/if}

<AlertDialog.Root open={leaving !== null} onOpenChange={(open) => !open && (leaving = null)}>
	<AlertDialog.Content>
		<AlertDialog.Header>
			<AlertDialog.Title>Leave without saving?</AlertDialog.Title>
			<AlertDialog.Description>Your changes to {what} haven't been saved. If you leave now, they're lost.</AlertDialog.Description>
		</AlertDialog.Header>
		<AlertDialog.Footer>
			<AlertDialog.Cancel>Stay</AlertDialog.Cancel>
			<AlertDialog.Action variant="outline" onclick={leave}>Leave without saving</AlertDialog.Action>
		</AlertDialog.Footer>
	</AlertDialog.Content>
</AlertDialog.Root>
