<!-- A list with nothing in it yet: what will be here, and the one action that fills it (D-059, rule 4). -->
<script lang="ts">
	import FileTextIcon from '@lucide/svelte/icons/file-text';
	import GlobeIcon from '@lucide/svelte/icons/globe';
	import HistoryIcon from '@lucide/svelte/icons/history';
	import ImageIcon from '@lucide/svelte/icons/image';
	import KeyRoundIcon from '@lucide/svelte/icons/key-round';
	import LayersIcon from '@lucide/svelte/icons/layers';
	import ListChecksIcon from '@lucide/svelte/icons/list-checks';
	import ListTodoIcon from '@lucide/svelte/icons/list-todo';
	import MailIcon from '@lucide/svelte/icons/mail';
	import MessageSquareIcon from '@lucide/svelte/icons/message-square';
	import PackageCheckIcon from '@lucide/svelte/icons/package-check';
	import PenLineIcon from '@lucide/svelte/icons/pen-line';
	import RouteIcon from '@lucide/svelte/icons/route';
	import SearchIcon from '@lucide/svelte/icons/search';
	import ShapesIcon from '@lucide/svelte/icons/shapes';
	import UserPlusIcon from '@lucide/svelte/icons/user-plus';
	import UsersRoundIcon from '@lucide/svelte/icons/users-round';
	import type { Component, Snippet } from 'svelte';
	import PixelIcon, { type PixelName } from './PixelIcon.svelte';

	let {
		icon,
		pixel,
		title,
		children,
		action,
		testid
	}: { icon?: Component; pixel?: PixelName; title: string; children?: Snippet; action?: Snippet; testid?: string } = $props();

	// Each list's icon in the website's pixel set (D-210 addendum, Studio v2); a caller may name one outright.
	const PIXEL = new Map<Component, PixelName>([
		[FileTextIcon, 'pen'], [PenLineIcon, 'pen'], [ImageIcon, 'image'], [SearchIcon, 'search'],
		[ListTodoIcon, 'check'], [ListChecksIcon, 'check'], [UserPlusIcon, 'people'], [UsersRoundIcon, 'people'],
		[MailIcon, 'people'], [LayersIcon, 'stamp'], [PackageCheckIcon, 'receipt'], [HistoryIcon, 'receipt'],
		[ShapesIcon, 'blocks'], [RouteIcon, 'blocks'], [GlobeIcon, 'globe'], [KeyRoundIcon, 'key'], [MessageSquareIcon, 'speech']
	]);
	const shown = $derived(pixel ?? (icon ? PIXEL.get(icon) : undefined) ?? 'empty');
</script>

<!-- The website's pixel icon (it wiggles on hover or tap) on the section ground, and the one line in Geist Pixel
     (D-210 addendum, Studio v2). -->
<div class="flex flex-col items-center rounded-card border border-border/70 bg-surface-section px-6 py-12 text-center" data-testid={testid}>
	<PixelIcon name={shown} size="2.5rem" wiggle class="mb-4" />
	<!-- The title in Geist Pixel, the product's own face (D-210); the explanation under it stays Inter. -->
	<p class="font-title text-2xl">{title}</p>
	{#if children}<div class="mt-1 max-w-md text-sm text-pretty text-muted-foreground">{@render children()}</div>{/if}
	{#if action}<div class="mt-5 flex flex-wrap justify-center gap-2">{@render action()}</div>{/if}
</div>
