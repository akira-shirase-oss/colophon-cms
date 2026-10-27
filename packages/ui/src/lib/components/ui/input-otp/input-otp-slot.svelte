<script lang="ts">
	import { PinInput as InputOTPPrimitive } from "bits-ui";
	import { cn } from "#lib/utils.js";

	let {
		ref = $bindable(null),
		cell,
		class: className,
		...restProps
	}: InputOTPPrimitive.CellProps = $props();
</script>

<InputOTPPrimitive.Cell
	{cell}
	bind:ref
	data-slot="input-otp-slot"
	class={cn(
		"border-input bg-card data-[active=true]:border-ring data-[active=true]:ring-4 data-[active=true]:ring-ring/15 aria-invalid:border-destructive size-11 rounded-lg border text-sm transition-[color,box-shadow,border-color] outline-none relative flex items-center justify-center data-[active=true]:z-10",
		className
	)}
	{...restProps}
>
	{#snippet child({ props })}
		<div {...props} data-active={cell.isActive}>
			{cell.char}
			{#if cell.hasFakeCaret}
				<div
					class="pointer-events-none absolute inset-0 flex items-center justify-center"
				>
					<div class="animate-caret-blink bg-foreground h-4 w-px duration-1000"></div>
				</div>
			{/if}
		</div>
	{/snippet}
</InputOTPPrimitive.Cell>
