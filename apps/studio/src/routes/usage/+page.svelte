<!--
  /studio/usage (D-235): what the signed-in account uses against its limits, across the projects
  it owns. A `null` limit is unbounded (administrators): the row shows the count and no meter.
-->
<script lang="ts">
	import { resolve } from '$app/paths';
	import { createQuery } from '@tanstack/svelte-query';
	import { Button } from '@colophon/ui/ui/button';
	import { Progress } from '@colophon/ui/ui/progress';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { me, usage, type Meter } from '#lib/api.ts';
	import Problem from '#lib/shell/Problem.svelte';

	const who = createQuery(() => ({ queryKey: ['me'], queryFn: me }));
	const used = createQuery(() => ({ queryKey: ['usage'], queryFn: usage, enabled: who.data != null }));

	const count = new Intl.NumberFormat();
	function bytes(n: number): string {
		const units = ['bytes', 'KB', 'MB', 'GB', 'TB'];
		let value = n;
		let unit = 0;
		while (value >= 1000 && unit < units.length - 1) {
			value /= 1000;
			unit += 1;
		}
		return unit === 0 ? `${count.format(value)} bytes` : `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
	}

	const rows = $derived(
		used.data
			? ([
					['Projects', used.data.projects, count.format],
					['Entries', used.data.entries, count.format],
					['Files', used.data.assets, count.format],
					['Storage', used.data.storage_bytes, bytes]
				] as [string, Meter, (n: number) => string][])
			: []
	);
</script>

<svelte:head><title>Usage · Colophon Studio</title></svelte:head>

<div class="mx-auto max-w-xl py-8">
	<h1 class="page-title">Usage</h1>
	<p class="mt-2 text-sm text-pretty text-muted-foreground">What your account uses across the projects you own, and how much it may.</p>
	<div class="mt-8">
		{#if who.isPending || (who.data && used.isPending)}
			<div class="space-y-6" aria-busy="true" data-testid="usage-loading">
				{#each [0, 1, 2, 3] as row (row)}<Skeleton class="h-10 w-full" />{/each}
			</div>
		{:else if who.isError}
			<Problem error={who.error} />
		{:else if who.data === null}
			<p class="text-sm" data-testid="usage-signed-out">Sign in to see your usage.</p>
			<div class="mt-4"><Button href={`${resolve('/signin')}?return_to=${encodeURIComponent(resolve('/usage'))}`}>Sign in</Button></div>
		{:else if used.isError}
			<Problem error={used.error} />
		{:else}
			<ul class="space-y-6 rounded-2xl border border-border bg-card p-6" data-testid="usage">
				{#each rows as [label, meter, show] (label)}
					{@const full = meter.limit !== null && meter.used >= meter.limit}
					<li data-testid={`usage-${label.toLowerCase()}`}>
						<div class="flex items-baseline justify-between gap-4 text-sm">
							<span class="font-medium">{label}</span>
							<span class={full ? 'text-destructive' : 'text-muted-foreground'}>
								{show(meter.used)}{meter.limit === null ? '' : ` of ${show(meter.limit)}`}
							</span>
						</div>
						{#if meter.limit === null}
							<p class="mt-1 text-sm text-muted-foreground">No limit</p>
						{:else}
							<Progress class="mt-2" value={Math.min(meter.used, meter.limit)} max={Math.max(meter.limit, 1)} aria-label={`${label}: ${show(meter.used)} of ${show(meter.limit)}`} />
							{#if full}<p class="mt-1 text-sm text-destructive">At the limit. Delete something, or ask the person who runs this Colophon for more.</p>{/if}
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>
