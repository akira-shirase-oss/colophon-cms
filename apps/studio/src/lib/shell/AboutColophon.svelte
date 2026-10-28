<!--
  Which Colophon this is, for a problem report (gap audit, 2026-10-28): the server's version and
  the day it was built, from `GET /version`, with one button that copies everything a helper needs
  (version, build, address, the page, the browser). Opened from the account menu, so every role
  finds it on every screen, Settings or not. The commit and API versions are technical details.
-->
<script lang="ts">
	import CopyIcon from '@lucide/svelte/icons/copy';
	import { createQuery } from '@tanstack/svelte-query';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import * as Dialog from '@colophon/ui/ui/dialog';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { serverVersion } from '#lib/api.ts';
	import Problem from './Problem.svelte';
	import TechnicalDetails from './TechnicalDetails.svelte';

	let { open = $bindable(false) }: { open?: boolean } = $props();

	// Asked when the dialog opens; a server keeps its version until it restarts.
	const version = createQuery(() => ({ queryKey: ['version'], queryFn: serverVersion, enabled: open, staleTime: 60_000 }));

	/** `2026-10-28` → "28 October 2026" in the reader's language; the server's `unknown` → said so. */
	function built(day: string): string {
		if (!/^\d{4}-\d{2}-\d{2}$/.test(day)) return 'Not recorded';
		return new Date(`${day}T00:00:00Z`).toLocaleDateString(undefined, { dateStyle: 'long', timeZone: 'UTC' });
	}

	async function copy() {
		const about = version.data;
		if (about === undefined) return;
		const text = [
			`Colophon ${about.version}`,
			`Built: ${about.build_date}`,
			`Commit: ${about.commit}`,
			`API versions: ${about.api.join(', ')}`,
			`Address: ${window.location.origin}`,
			`Page: ${window.location.href}`,
			`Browser: ${navigator.userAgent}`
		].join('\n');
		try {
			await navigator.clipboard.writeText(text);
			toast.success('Copied. Paste it into your message.');
		} catch {
			toast.error("Couldn't copy; select the details and copy them yourself.");
		}
	}
</script>

<Dialog.Root bind:open>
	<Dialog.Content class="sm:max-w-md" data-testid="about">
		<Dialog.Header>
			<Dialog.Title>About Colophon</Dialog.Title>
			<Dialog.Description>
				Reporting a problem? Copy these details into your message, so whoever helps knows exactly which Colophon you were using.
			</Dialog.Description>
		</Dialog.Header>
		{#if version.isPending}
			<Skeleton class="h-24 w-full rounded-xl" />
		{:else if version.isError}
			<Problem error={version.error} what="the version details" retry={() => version.refetch()} />
		{:else}
			<dl class="space-y-2.5 rounded-xl border border-border bg-card px-4 py-3 text-sm">
				<div class="flex items-baseline justify-between gap-4">
					<dt class="text-muted-foreground">Version</dt>
					<dd class="font-medium tabular" data-testid="about-version">{version.data.version}</dd>
				</div>
				<div class="flex items-baseline justify-between gap-4">
					<dt class="text-muted-foreground">Built on</dt>
					<dd data-testid="about-built">{built(version.data.build_date)}</dd>
				</div>
				<div class="flex min-w-0 items-baseline justify-between gap-4">
					<dt class="shrink-0 text-muted-foreground">Address</dt>
					<dd class="min-w-0 truncate font-mono text-xs">{window.location.origin}</dd>
				</div>
			</dl>
			<TechnicalDetails>
				<p class="break-all">Commit <span class="font-mono" data-testid="about-commit">{version.data.commit}</span></p>
				<p>API versions <span class="font-mono">{version.data.api.join(', ')}</span></p>
				<p>From <span class="font-mono">GET /version</span>, which needs no sign-in.</p>
			</TechnicalDetails>
		{/if}
		<Dialog.Footer>
			<Button onclick={copy} disabled={version.data === undefined} data-testid="about-copy"><CopyIcon data-icon="inline-start" />Copy details</Button>
		</Dialog.Footer>
	</Dialog.Content>
</Dialog.Root>
