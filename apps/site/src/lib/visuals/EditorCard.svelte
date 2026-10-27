<!-- The entry editor with autosave (P1): a save names its version; unsaved edits stay on the device. -->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import { Badge } from '@colophon/ui/ui/badge';

	let { class: className = '' }: { class?: string } = $props();
</script>

<div class="rounded-3xl border border-border/60 bg-card p-5 text-left shadow-float {className}" aria-hidden="true">
	<div class="flex items-center justify-between gap-3">
		<p class="eyebrow">Articles · Draft</p>
		<Badge variant="live"><CheckIcon />Saved · version 7</Badge>
	</div>
	<p class="mt-4 text-[0.6875rem] font-medium text-muted-foreground">Title</p>
	<div class="mt-1 rounded-xl border border-input bg-card px-3 py-2 text-sm font-medium shadow-hairline">Spring menu</div>
	<p class="mt-3 text-[0.6875rem] font-medium text-muted-foreground">Body</p>
	<div class="mt-1 space-y-2 rounded-xl border border-input bg-card px-3 py-3 shadow-hairline">
		<span class="block h-2 w-11/12 rounded-full bg-muted"></span>
		<span class="block h-2 w-10/12 rounded-full bg-muted"></span>
		<span class="block h-2 w-7/12 rounded-full bg-muted"></span>
	</div>
	<p class="mt-3 text-xs text-muted-foreground">A copy of unsaved edits stays on this device.</p>
</div>
