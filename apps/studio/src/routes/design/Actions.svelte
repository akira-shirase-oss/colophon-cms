<script lang="ts">
	import BoldIcon from '@lucide/svelte/icons/bold';
	import ItalicIcon from '@lucide/svelte/icons/italic';
	import UnderlineIcon from '@lucide/svelte/icons/underline';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import SendIcon from '@lucide/svelte/icons/send';
	import ChevronDownIcon from '@lucide/svelte/icons/chevron-down';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button, buttonVariants } from '@colophon/ui/ui/button';
	import * as ButtonGroup from '@colophon/ui/ui/button-group';
	import * as Kbd from '@colophon/ui/ui/kbd';
	import { Spinner } from '@colophon/ui/ui/spinner';
	import { Toggle } from '@colophon/ui/ui/toggle';
	import * as ToggleGroup from '@colophon/ui/ui/toggle-group';
	import * as Tooltip from '@colophon/ui/ui/tooltip';
	import Specimen from './Specimen.svelte';

	const VARIANTS = ['default', 'outline', 'secondary', 'ghost', 'destructive', 'link'] as const;
	const BADGES = ['default', 'secondary', 'outline', 'brand', 'live', 'pending', 'attention', 'draft', 'destructive'] as const;
	// Whole class names, so Tailwind finds them.
	const DOT: Partial<Record<(typeof BADGES)[number], string>> = {
		live: 'bg-live',
		pending: 'bg-pending breathe',
		attention: 'bg-attention',
		draft: 'bg-draft'
	};
	let mark = $state('bold');
</script>

<div class="grid gap-12">
	<Specimen label="Button variants">
		<div class="flex flex-wrap items-center gap-3">
			{#each VARIANTS as variant (variant)}
				<Button {variant}>{variant === 'default' ? 'Publish release' : variant[0]?.toUpperCase() + variant.slice(1)}</Button>
			{/each}
		</div>
	</Specimen>

	<Specimen label="Sizes, icons and states">
		<div class="flex flex-wrap items-center gap-3">
			<Button size="lg">Create account</Button>
			<Button>Save draft</Button>
			<Button size="sm" variant="outline"><PlusIcon data-icon="inline-start" />New entry</Button>
			<Button size="xs" variant="secondary">Small</Button>
			<Button variant="outline" size="icon" aria-label="Send"><SendIcon /></Button>
			<Button variant="ghost" size="icon-sm" aria-label="Delete"><Trash2Icon /></Button>
			<Button disabled><Spinner />Publishing…</Button>
			<Button variant="outline" disabled>Disabled</Button>
		</div>
	</Specimen>

	<Specimen label="Social sign-in, full width">
		<div class="grid max-w-md grid-cols-2 gap-3">
			<Button variant="outline" size="lg">Keycloak</Button>
			<Button variant="outline" size="lg">GitHub</Button>
			<Button size="lg" class="col-span-2">Continue</Button>
		</div>
	</Specimen>

	<Specimen label="Badges and status pills">
		<div class="flex flex-wrap items-center gap-2">
			{#each BADGES as variant (variant)}
				<Badge {variant}>
					{#if DOT[variant]}<span class="size-1.5 rounded-full {DOT[variant]}"></span>{/if}
					{variant === 'default' ? 'Release 42' : variant[0]?.toUpperCase() + variant.slice(1)}
				</Badge>
			{/each}
		</div>
	</Specimen>

	<div class="grid gap-12 md:grid-cols-2">
		<Specimen label="Toggle and toggle group">
			<div class="flex flex-wrap items-center gap-3">
				<Toggle aria-label="Bold"><BoldIcon /></Toggle>
				<Toggle variant="outline" aria-label="Italic"><ItalicIcon /></Toggle>
				<ToggleGroup.Root type="single" variant="outline" bind:value={mark} aria-label="Formatting">
					<ToggleGroup.Item value="bold" aria-label="Bold"><BoldIcon /></ToggleGroup.Item>
					<ToggleGroup.Item value="italic" aria-label="Italic"><ItalicIcon /></ToggleGroup.Item>
					<ToggleGroup.Item value="underline" aria-label="Underline"><UnderlineIcon /></ToggleGroup.Item>
				</ToggleGroup.Root>
			</div>
		</Specimen>

		<Specimen label="Button group">
			<ButtonGroup.Root>
				<Button variant="outline">Save</Button>
				<Button variant="outline">Preview</Button>
				<Button variant="outline" size="icon" aria-label="More"><ChevronDownIcon /></Button>
			</ButtonGroup.Root>
		</Specimen>

		<Specimen label="Tooltip">
			<Tooltip.Root>
				<Tooltip.Trigger class={buttonVariants({ variant: 'outline' })}>Hover me</Tooltip.Trigger>
				<Tooltip.Content>Gen 42 · verified</Tooltip.Content>
			</Tooltip.Root>
		</Specimen>

		<Specimen label="Keyboard">
			<p class="flex items-center gap-2 text-sm text-muted-foreground">
				Search titles with
				<Kbd.Group><Kbd.Root>⌘</Kbd.Root><Kbd.Root>K</Kbd.Root></Kbd.Group>
			</p>
		</Specimen>
	</div>
</div>
