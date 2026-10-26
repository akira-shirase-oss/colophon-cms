<!-- A toolbar is one tab stop; arrow keys move inside it and wrap around (WAI-ARIA toolbar). -->
<script lang="ts">
	import { Toolbar as ToolbarPrimitive } from "bits-ui";
	import { cn } from "#lib/utils.js";

	let { ref = $bindable(null), class: className, loop = true, ...restProps }: ToolbarPrimitive.RootProps = $props();
</script>

<ToolbarPrimitive.Root
	bind:ref
	{loop}
	data-slot="toolbar"
	class={cn("flex flex-wrap items-center gap-0.5 border-b border-border bg-muted/40 px-1.5 py-1", className)}
	{...restProps}
/>
