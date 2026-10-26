<script lang="ts">
	import { Toolbar as ToolbarPrimitive } from "bits-ui";
	import { cn } from "#lib/utils.js";

	let {
		ref = $bindable(null),
		class: className,
		pressed,
		...restProps
	}: ToolbarPrimitive.ButtonProps & { pressed?: boolean } = $props();
</script>

<ToolbarPrimitive.Button
	bind:ref
	data-slot="toolbar-button"
	aria-pressed={pressed}
	class={cn(
		"inline-flex h-8 min-w-8 items-center justify-center gap-1 rounded-md px-2 text-sm text-muted-foreground outline-none transition-colors hover:bg-background hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 aria-pressed:bg-background aria-pressed:text-foreground aria-pressed:shadow-xs disabled:opacity-50 [&_svg]:size-4 [&_svg]:shrink-0",
		className
	)}
	{...restProps}
/>
