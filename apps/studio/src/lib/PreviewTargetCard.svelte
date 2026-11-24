<!--
  Settings → Website connections → Preview on your site (D-181): one address
  template, where an environment's draft previews open. Colophon fills in a one-time code (and,
  if asked, the entry's collection, id, URL name and locale); the site's preview route trades
  the code for access and shows the draft. The checks here are the server's, mirrored, so a
  mistake is said while typing; the server still decides, and its refusal is said plainly too.
  Roles without `destinations.manage` read where previews open, and are told who can change it.
-->
<script lang="ts">
	import EyeIcon from '@lucide/svelte/icons/eye';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError } from './api.ts';
	import { describe } from './errors.ts';
	import { getPreviewTarget, MAX_TEMPLATE, setPreviewTarget, TEMPLATE_EXAMPLE, templateOrigin, templateProblem } from './preview.ts';
	import { ago } from './queries.ts';
	import Problem from './shell/Problem.svelte';
	import TechnicalDetails from './shell/TechnicalDetails.svelte';
	import { WEBSITE_CONNECTIONS } from './words.ts';

	let { project, manages }: { project: string; manages: boolean } = $props();

	const queryClient = useQueryClient();
	const target = createQuery(() => ({ queryKey: ['preview-target', project], queryFn: () => getPreviewTarget(project) }));
	const saved = $derived(target.data?.url_template ?? '');

	let address = $state('');
	/** The field shows what is saved until someone types in it. */
	let edited = $state(false);
	$effect(() => {
		if (!edited) address = saved;
	});
	/** Checked as you type once you've left the field or tried to save. */
	let touched = $state(false);
	let busy = $state<'save' | 'clear' | null>(null);
	/** The server's refusal: its words, plainly, and the codes for Technical details. */
	let refused = $state<{ text: string; technical: string } | null>(null);

	const problem = $derived(touched ? templateProblem(address) : '');
	const origin = $derived(templateOrigin(address));
	const shown = $derived(refused?.text ?? problem);
	const changed = $derived(address.trim() !== saved);

	function said(error: unknown): { text: string; technical: string } {
		const plain = describe(error, `manage ${WEBSITE_CONNECTIONS}`);
		// The server's refusal of an address is written for people (it names the rule), so it is
		// shown as it is; anything else in the Studio's words.
		if (error instanceof ApiError && error.status === 422 && error.message !== '') {
			const text = error.message.slice(0, 1).toUpperCase() + error.message.slice(1);
			return { text: /[.!?]$/.test(text) ? text : `${text}.`, technical: plain.technical };
		}
		return { text: plain.text, technical: plain.technical };
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		touched = true;
		refused = null;
		if (templateProblem(address) !== '') {
			document.getElementById('preview-address')?.focus();
			return;
		}
		busy = 'save';
		try {
			const next = await setPreviewTarget(project, address.trim());
			queryClient.setQueryData(['preview-target', project], next);
			edited = false;
			touched = false;
			toast.success('Saved. Editors can preview drafts on your site.');
		} catch (error) {
			refused = said(error);
			document.getElementById('preview-address')?.focus();
		} finally {
			busy = null;
		}
	}

	async function clear() {
		refused = null;
		busy = 'clear';
		try {
			const next = await setPreviewTarget(project, null);
			queryClient.setQueryData(['preview-target', project], next);
			edited = false;
			touched = false;
			toast.success('Preview on your site is off.');
		} catch (error) {
			refused = said(error);
		} finally {
			busy = null;
		}
	}
</script>

<section id="preview" class="mt-12 scroll-mt-20" aria-labelledby="preview-target-heading" data-testid="preview-target">
	<div class="mb-3 flex flex-wrap items-center gap-x-3 gap-y-1">
		<h2 id="preview-target-heading" class="text-lg font-semibold tracking-tight">Preview on your site</h2>
		{#if target.isSuccess}
			{#if saved !== ''}
				<Badge variant="live" data-testid="preview-target-state"><span class="size-1.5 rounded-full bg-live" aria-hidden="true"></span>On</Badge>
			{:else}
				<Badge variant="draft" data-testid="preview-target-state"><span class="size-1.5 rounded-full bg-draft" aria-hidden="true"></span>Not set up</Badge>
			{/if}
		{/if}
	</div>
	<p class="mb-4 text-sm text-pretty text-muted-foreground">
		Show drafts on your own site before they go live. Your site needs a preview route — the SDK does it in about 10 lines.
	</p>

	{#if target.isError}
		<Problem error={target.error} what="the preview address" retry={() => target.refetch()} />
	{:else if target.isPending}
		<Skeleton class="h-[23rem] w-full rounded-xl" />
	{:else if !manages}
		<p class="rounded-lg border border-border bg-card px-4 py-3 text-sm text-muted-foreground" data-testid="preview-target-readonly">
			{#if saved !== '' && target.data.origin}
				Editors can preview drafts on <strong class="font-medium text-foreground">{target.data.origin}</strong>. Only owners and developers change where; ask an owner if it should
				be somewhere else.
			{:else}
				Not set up yet. Ask an owner of this project to add your site’s preview address.
			{/if}
		</p>
	{:else}
		<form class="rounded-xl border border-border bg-card p-5" onsubmit={save} aria-labelledby="preview-target-heading" novalidate>
			<div class="grid gap-1.5">
				<label class="text-sm font-medium" for="preview-address">Preview address</label>
				<Input
					id="preview-address"
					class="font-mono text-sm"
					bind:value={() => address, (next) => ((address = next), (edited = true), (refused = null))}
					onblur={() => (touched = touched || address.trim() !== '')}
					placeholder={TEMPLATE_EXAMPLE}
					autocomplete="off"
					spellcheck={false}
					maxlength={MAX_TEMPLATE + 200}
					aria-invalid={shown !== ''}
					aria-describedby="preview-address-help{shown !== '' ? ' preview-address-problem' : ''}"
					data-testid="preview-address"
				/>
				{#if shown !== ''}
					<p id="preview-address-problem" class="text-sm text-destructive" role="alert" data-testid="preview-address-problem">{shown}</p>
					{#if refused !== null}
						<TechnicalDetails><p class="break-words">{refused.technical}</p></TechnicalDetails>
					{/if}
				{/if}
				<div id="preview-address-help" class="mt-1 space-y-2 text-xs text-muted-foreground">
					<p>Your site’s preview route, with these filled in for each entry:</p>
					<dl class="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
						<dt><code class="font-mono text-foreground">{'{code}'}</code></dt>
						<dd>Required. A one-time code your route trades for access to the draft.</dd>
						<dt><code class="font-mono text-foreground">{'{collection}'}</code></dt>
						<dd>The entry’s collection, like <span class="font-mono">articles</span>.</dd>
						<dt><code class="font-mono text-foreground">{'{id}'}</code></dt>
						<dd>The entry’s id.</dd>
						<dt><code class="font-mono text-foreground">{'{slug}'}</code></dt>
						<dd>Its URL name, when it has one.</dd>
						<dt><code class="font-mono text-foreground">{'{locale}'}</code></dt>
						<dd>Its language, like <span class="font-mono">en</span>.</dd>
					</dl>
					<p>Use an https:// address; plain http:// works only for localhost while you develop.</p>
				</div>
			</div>
			<div class="mt-5 flex flex-wrap items-center gap-2 border-t border-dotted border-border pt-4">
				<Button type="submit" size="sm" disabled={busy !== null || !changed} data-testid="preview-address-save">{busy === 'save' ? 'Saving…' : 'Save'}</Button>
				{#if saved !== ''}
					<Button type="button" size="sm" variant="outline" disabled={busy !== null} onclick={clear} data-testid="preview-address-clear"
						>{busy === 'clear' ? 'Turning off…' : 'Turn off'}</Button
					>
				{/if}
				<p class="min-w-0 flex-1 text-xs text-muted-foreground sm:text-right" data-testid="preview-target-summary">
					{#if saved !== '' && !changed && target.data.origin}
						<EyeIcon class="mr-1 inline size-3.5 align-[-0.15em]" aria-hidden="true" />Previews open on <span class="font-medium text-foreground">{target.data.origin}</span
						>{#if target.data.updated_at}<span title={new Date(target.data.updated_at).toLocaleString()}>, saved {ago(target.data.updated_at)}</span>{/if}.
					{:else if changed && origin !== null}
						Previews will open on <span class="font-medium text-foreground">{origin}</span>.
					{/if}
				</p>
			</div>
		</form>
	{/if}
</section>
