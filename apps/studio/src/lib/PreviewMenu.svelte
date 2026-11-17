<!--
  The entry editor's Preview button (D-178): a menu with "Side by side" (the site in a
  pane beside the fields; Edit and Preview tabs on a phone) and "Open on my site" (a new tab). When
  nobody has set the site's preview address yet, the button is shown disabled with the reason and,
  for roles that may set it, a link to where.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import Columns2Icon from '@lucide/svelte/icons/columns-2';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import EyeIcon from '@lucide/svelte/icons/eye';
	import XIcon from '@lucide/svelte/icons/x';
	import { createQuery } from '@tanstack/svelte-query';
	import { tick } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { toast } from 'svelte-sonner';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';
	import { getPreviewTarget, issuePreview, NARROW_QUERY, openInNewTab, previewProblem } from './preview.ts';

	let {
		project,
		entry,
		open = $bindable(false),
		manages
	}: {
		project: string;
		entry: string;
		/** Whether the preview pane is showing. */
		open?: boolean;
		/** Whether this role may set the preview address (`destinations.manage`). */
		manages: boolean;
	} = $props();

	// Any member who reads content may ask whether preview is set up (the same key Settings uses,
	// so saving the address there updates this button).
	const target = createQuery(() => ({ queryKey: ['preview-target', project], queryFn: () => getPreviewTarget(project), staleTime: 60_000 }));
	const ready = $derived((target.data?.url_template ?? '') !== '');
	const narrow = new MediaQuery(NARROW_QUERY);
	const settingsHref = $derived(`${resolve('/projects/[project]/settings/connections', { project })}#preview`);

	// Opening side by side moves focus into the pane (it says what it is doing there); closing it
	// brings focus back here, rather than leaving it on the page's body.
	let trigger = $state<HTMLElement | null>(null);
	let shown = false;
	$effect(() => {
		const now = open;
		if (shown && !now) {
			void tick().then(() => {
				if (document.activeElement === null || document.activeElement === document.body) trigger?.focus();
			});
		}
		shown = now;
	});

	async function openOnSite() {
		const failure = await openInNewTab(() => issuePreview(project, entry));
		if (failure !== null) {
			const said = previewProblem(failure);
			toast.error(said.title, { description: said.text });
		}
	}
</script>

{#if ready}
	<DropdownMenu.Root>
		<DropdownMenu.Trigger bind:ref={trigger} class={buttonVariants({ variant: open ? 'secondary' : 'outline', size: 'sm' })} data-testid="preview-menu">
			<EyeIcon />Preview<ChevronDownIcon class="-mr-1 text-muted-foreground" />
		</DropdownMenu.Trigger>
		<DropdownMenu.Content align="end" class="w-64" onCloseAutoFocus={(event) => open && event.preventDefault()}>
			{#if open}
				<DropdownMenu.Item onSelect={() => (open = false)} data-testid="preview-menu-close"><XIcon class="text-muted-foreground" />Close the preview</DropdownMenu.Item>
			{:else}
				<DropdownMenu.Item class="items-start" onSelect={() => (open = true)} data-testid="preview-menu-side">
					<Columns2Icon class="mt-0.5 text-muted-foreground" />
					<span class="grid gap-0.5">
						<span>{narrow.current ? 'In this page' : 'Side by side'}</span>
						<span class="font-normal text-muted-foreground">{narrow.current ? 'Switch between Edit and Preview' : 'Your site beside the fields'}</span>
					</span>
				</DropdownMenu.Item>
			{/if}
			<DropdownMenu.Item class="items-start" onSelect={openOnSite} data-testid="preview-menu-site">
				<ExternalLinkIcon class="mt-0.5 text-muted-foreground" />
				<span class="grid gap-0.5">
					<span>Open on my site</span>
					<span class="font-normal text-muted-foreground">In a new tab, as your site shows it</span>
				</span>
			</DropdownMenu.Item>
		</DropdownMenu.Content>
	</DropdownMenu.Root>
{:else}
	<!-- Disabled until the site's preview address is set, with the reason beside it (a disabled
	     button takes no focus, so the reason is text, and the way out is a link). -->
	<div class="flex min-h-9 flex-wrap items-center justify-end gap-x-2.5 gap-y-1" data-testid="preview-unavailable">
		{#if target.isError}
			<span class="text-xs text-muted-foreground" id="preview-reason-{entry}">Couldn’t check whether preview is set up.</span>
			<Button variant="link" size="xs" class="h-auto px-0" onclick={() => target.refetch()}>Try again</Button>
		{:else if target.isSuccess}
			<span class="text-xs text-muted-foreground" id="preview-reason-{entry}">
				{#if manages}
					Preview on your site isn’t set up. <a class="font-medium text-foreground underline underline-offset-2" href={settingsHref} data-testid="preview-set-up">Set it up</a>
				{:else}
					Preview on your site isn’t set up: ask an owner.
				{/if}
			</span>
		{/if}
		<!-- While checking, it looks like the menu button it will most likely become: no jump. -->
		<Button variant="outline" size="sm" disabled aria-describedby={target.isPending ? undefined : `preview-reason-${entry}`} aria-busy={target.isPending} data-testid="preview-menu"
			><EyeIcon />Preview{#if target.isPending}<ChevronDownIcon class="-mr-1 text-muted-foreground" />{/if}</Button
		>
	</div>
{/if}
