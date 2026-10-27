<!-- A publish request with its review (P2): the approval is for this exact set of versions. -->
<script lang="ts">
	import ShieldCheckIcon from '@lucide/svelte/icons/shield-check';
	import { Badge } from '@colophon/ui/ui/badge';

	let { class: className = '' }: { class?: string } = $props();

	const ITEMS = [
		{ title: 'Spring menu', version: 7 },
		{ title: 'Opening hours', version: 3 },
		{ title: 'About us', version: 12 }
	];
</script>

<div class="rounded-3xl border border-border/60 bg-card p-5 text-left shadow-float {className}" aria-hidden="true">
	<div class="flex items-center justify-between gap-3">
		<p class="text-sm font-semibold">Publish request</p>
		<Badge variant="live"><ShieldCheckIcon />Approved</Badge>
	</div>
	<ul class="mt-4 divide-y divide-border rounded-2xl border border-border">
		{#each ITEMS as item (item.title)}
			<li class="flex items-center justify-between gap-3 px-3 py-2 text-sm">
				<span class="truncate">{item.title}</span>
				<span class="figure shrink-0 text-xs text-muted-foreground">version {item.version}</span>
			</li>
		{/each}
	</ul>
	<p class="mt-3 text-xs text-muted-foreground">Approved for these exact versions. Edit one, and it needs approving again.</p>
	<div class="mt-4 flex items-center justify-between gap-3">
		<code class="font-mono text-[0.6875rem] text-muted-foreground">hash 4be1…07c2</code>
		<span class="inline-flex h-9 items-center rounded-full bg-primary px-4 text-sm font-medium text-primary-foreground shadow-press">Publish</span>
	</div>
</div>
