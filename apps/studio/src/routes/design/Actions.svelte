<script lang="ts">
	import BoldIcon from '@lucide/svelte/icons/bold';
	import DownloadIcon from '@lucide/svelte/icons/download';
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

	/** The hierarchy (D-182): what each is for. `outline` is another name for secondary. */
	const VARIANTS = [
		{ variant: 'default', label: 'Publish', use: 'Primary: the one thing this screen is for' },
		{ variant: 'secondary', label: 'Preview', use: 'Secondary: everything else' },
		{ variant: 'ghost', label: 'Clear', use: 'Ghost: quiet, in toolbars and rows' },
		{ variant: 'destructive', label: 'Unpublish', use: "Destructive: a confirmation's last step" },
		{ variant: 'link', label: 'The publish request', use: 'Link: a place, inside a sentence' }
	] as const;
	const BADGES = ['secondary', 'outline', 'brand', 'live', 'changed', 'pending', 'attention', 'draft'] as const;
	let mark = $state('bold');
</script>

<div class="grid gap-12">
	<Specimen label="The hierarchy">
		<div class="grid gap-3 sm:grid-cols-2 lg:grid-cols-5">
			{#each VARIANTS as { variant, label, use } (variant)}
				<div class="grid content-start justify-items-start gap-2">
					<Button {variant}>{label}</Button>
					<p class="text-sm text-muted-foreground">{use}</p>
				</div>
			{/each}
		</div>
	</Specimen>

	<Specimen label="One primary per screen: a list page's header">
		<div class="flex flex-wrap items-center justify-between gap-3 rounded-card border border-border bg-card px-card py-4">
			<p class="page-title text-2xl">Articles</p>
			<div class="flex flex-wrap gap-2">
				<Button variant="outline"><DownloadIcon data-icon="inline-start" />Export</Button>
				<Button><PlusIcon data-icon="inline-start" />New entry</Button>
			</div>
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

	<Specimen label="Badges for labels that aren't a state (a state is a StatusBadge)">
		<div class="flex flex-wrap items-center gap-2">
			{#each BADGES as variant (variant)}
				<Badge {variant}>{variant[0]?.toUpperCase() + variant.slice(1)}</Badge>
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
