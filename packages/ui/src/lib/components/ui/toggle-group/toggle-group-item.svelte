<script lang="ts">
	import { ToggleGroup as ToggleGroupPrimitive } from "bits-ui";
	import { type ToggleVariants, toggleVariants } from "#lib/components/ui/toggle/index.js";
	import { cn } from "#lib/utils.js";
	import { getToggleGroupCtx } from "./toggle-group.svelte";

	let {
		ref = $bindable(null),
		value = $bindable(),
		class: className,
		size,
		variant,
		...restProps
	}: ToggleGroupPrimitive.ItemProps & ToggleVariants = $props();

	const ctx = getToggleGroupCtx();
</script>

<ToggleGroupPrimitive.Item
	bind:ref
	data-slot="toggle-group-item"
	data-variant={ctx.variant || variant}
	data-size={ctx.size || size}
	data-spacing={ctx.spacing}
	class={cn(
		toggleVariants({
			variant: ctx.variant || variant,
			size: ctx.size || size,
		}),
		"h-8 min-w-8 shrink-0 rounded-full border-0 px-2.5 shadow-none focus:z-10 focus-visible:z-10 data-[state=on]:bg-card data-[state=on]:text-foreground data-[state=on]:shadow-hairline group-data-[variant=outline]/toggle-group:data-[state=on]:bg-secondary group-data-[variant=outline]/toggle-group:data-[state=on]:shadow-none",
		className
	)}
	{value}
	{...restProps}
/>
