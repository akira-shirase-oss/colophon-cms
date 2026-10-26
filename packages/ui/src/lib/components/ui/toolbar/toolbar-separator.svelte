<script lang="ts">
	import { cn } from "#lib/utils.js";

	let { class: className }: { class?: string } = $props();
</script>

<div role="separator" aria-orientation="vertical" data-slot="toolbar-separator" class={cn("mx-1 h-5 w-px bg-border", className)}></div>
