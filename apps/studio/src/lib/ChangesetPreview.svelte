<!--
  "Preview on your site" for one publish request (branch gap/preview): a grant for exactly the
  versions it holds, bound to its hash, opened in a new tab on the editor's own site. If the
  request changes, the site's preview stops (it answers "stale") and this is pressed again; once
  the request is live there is nothing to preview, so the page doesn't offer it. Not set up, the
  button is disabled with the reason and, for roles that may set it, where.
-->
<script lang="ts">
	import EyeIcon from '@lucide/svelte/icons/eye';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import { resolve } from '$app/paths';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import type { Said } from './errors.ts';
	import { changesetPreviewProblem, getPreviewTarget, issueChangesetPreview, notConfigured, openInNewTab } from './preview.ts';
	import TechnicalDetails from './shell/TechnicalDetails.svelte';

	let {
		project,
		changeset,
		published,
		manages
	}: {
		project: string;
		changeset: string;
		/** Live already: nothing to preview. */
		published: boolean;
		/** Whether this role may set the preview address (`destinations.manage`). */
		manages: boolean;
	} = $props();

	const queryClient = useQueryClient();
	const target = createQuery(() => ({ queryKey: ['preview-target', project], queryFn: () => getPreviewTarget(project), staleTime: 60_000 }));
	const ready = $derived((target.data?.url_template ?? '') !== '');
	const settingsHref = $derived(`${resolve('/projects/[project]/settings/connections', { project })}#preview`);

	let opening = $state(false);
	/** Why the last try didn't open, in plain words; the server's in Technical details. */
	let problem = $state<Said | null>(null);
	/** The last try said nobody set the preview address (it was cleared since the page loaded). */
	let unset = $state(false);

	async function preview() {
		if (opening) return;
		opening = true;
		problem = null;
		unset = false;
		const failure = await openInNewTab(() => issueChangesetPreview(project, changeset));
		opening = false;
		if (failure === null) return;
		if (notConfigured(failure)) {
			unset = true;
			void queryClient.invalidateQueries({ queryKey: ['preview-target', project] });
			return;
		}
		problem = changesetPreviewProblem(failure);
	}
</script>

{#if !published || problem !== null}
	<section class="mb-8 rounded-xl border border-border bg-card px-4 py-3.5" aria-label="Preview on your site" data-testid="changeset-preview">
		<div class="flex flex-wrap items-center gap-x-4 gap-y-2">
			{#if !published}
				{#if target.isSuccess && ready && !unset}
					<!-- Not disabled while opening: a focused button that turns disabled drops the focus. -->
					<Button
						variant="outline"
						size="sm"
						class="min-w-46 aria-busy:opacity-70"
						onclick={preview}
						aria-busy={opening}
						aria-describedby="changeset-preview-note"
						data-testid="changeset-preview-button"
						>{#if opening}<LoaderIcon class="animate-spin" aria-hidden="true" />Opening…{:else}<EyeIcon aria-hidden="true" />Preview on your site{/if}</Button
					>
					<p id="changeset-preview-note" class="min-w-0 flex-1 basis-64 text-sm text-pretty text-muted-foreground">
						Shows exactly what this request will publish, for 15 minutes. If the request changes, preview it again.
					</p>
				{:else}
					<Button variant="outline" size="sm" disabled aria-busy={target.isPending} aria-describedby={target.isPending ? undefined : 'changeset-preview-reason'} data-testid="changeset-preview-button"
						><EyeIcon aria-hidden="true" />Preview on your site</Button
					>
					<p id="changeset-preview-reason" class="min-w-0 flex-1 basis-64 text-sm text-muted-foreground" data-testid="changeset-preview-reason">
						{#if target.isPending}
							<span class="sr-only">Checking whether preview is set up…</span>
						{:else if target.isError}
							Couldn’t check whether preview is set up.
							<button class="font-medium text-foreground underline underline-offset-2 outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40" onclick={() => target.refetch()}
								>Try again</button
							>
						{:else if manages}
							Preview on your site isn’t set up yet. <a class="font-medium text-foreground underline underline-offset-2" href={settingsHref} data-testid="changeset-preview-set-up"
								>Set it up in Website connections</a
							>
						{:else}
							Preview on your site isn’t set up yet: ask an owner of this project.
						{/if}
					</p>
				{/if}
			{/if}
		</div>
		{#if problem !== null}
			<div role="alert" class="mt-3 rounded-lg bg-attention-soft px-3 py-2.5 text-sm text-attention-ink" data-testid="changeset-preview-problem">
				<p class="font-medium">{problem.title}</p>
				<p class="mt-0.5">{problem.text}</p>
				<TechnicalDetails class="mt-1.5"><p class="break-words">{problem.technical}</p></TechnicalDetails>
			</div>
		{/if}
	</section>
{/if}
