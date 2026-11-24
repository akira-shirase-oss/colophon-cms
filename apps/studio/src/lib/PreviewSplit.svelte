<!--
  The editor beside its preview (D-181). Closed, it adds nothing: the editor renders
  as it always does. Open, the editor and the preview pane sit side by side with a handle between
  them that drags, and moves with the arrow keys (a separator, like a slider); at 640 px and under,
  they become two tabs, Edit and Preview. The editor stays mounted, in the same place, whatever
  the layout — its rich-text editors and its autosave are never rebuilt by opening the preview.
-->
<script lang="ts">
	import type { Snippet } from 'svelte';
	import { MediaQuery } from 'svelte/reactivity';
	import { NARROW_QUERY } from './preview.ts';

	let { open, id, children, pane }: { open: boolean; id: string; children: Snippet; pane: Snippet } = $props();

	const narrowQuery = new MediaQuery(NARROW_QUERY);
	const narrow = $derived(narrowQuery.current);
	const split = $derived(open && !narrow);
	const tabbed = $derived(open && narrow);

	// The editor's share of the width, in percent; remembered on this device.
	const MIN = 30;
	const MAX = 70;
	const STEP = 5;
	const KEY = 'colophon.preview.share';
	function remembered(): number {
		try {
			const value = Number(localStorage.getItem(KEY));
			return Number.isFinite(value) && value >= MIN && value <= MAX ? value : 45;
		} catch {
			return 45;
		}
	}
	let share = $state(remembered());
	function setShare(value: number) {
		share = Math.round(Math.min(MAX, Math.max(MIN, value)));
		try {
			localStorage.setItem(KEY, String(share));
		} catch {
			// Not remembered: fine.
		}
	}

	let tab = $state<'edit' | 'preview'>('preview');
	// Opening the preview shows it; on a phone, that means its tab.
	$effect(() => {
		if (open) tab = 'preview';
	});

	let container = $state<HTMLDivElement | null>(null);
	let dragging = $state(false);

	// The pane fills the screen below where it sits, to the bottom with a small margin: side by
	// side it sticks under the top bar as the fields scroll (and grows into the room the page
	// header leaves); as a tab it fills the screen below the tabs. Measured, so its bottom edge is
	// never under the fold and never leaves a gap.
	const TOP = 72;
	let paneBox = $state<HTMLDivElement | null>(null);
	let height = $state<number | null>(null);
	$effect(() => {
		if (!open || paneBox === null) return;
		const box = paneBox;
		const sticky = split;
		let frame = 0;
		const measure = () => {
			frame = 0;
			const top = box.getBoundingClientRect().top;
			if (box.hidden || box.offsetParent === null) return;
			const next = sticky ? window.innerHeight - Math.max(top, TOP) - 24 : window.innerHeight - (top + window.scrollY) - 16;
			height = Math.max(384, Math.round(next));
		};
		const later = () => {
			if (frame === 0) frame = requestAnimationFrame(measure);
		};
		measure();
		window.addEventListener('resize', later);
		if (sticky) window.addEventListener('scroll', later, { passive: true });
		return () => {
			cancelAnimationFrame(frame);
			window.removeEventListener('resize', later);
			window.removeEventListener('scroll', later);
		};
	});
	// Showing the Preview tab measures again (a hidden pane has no place to measure from).
	$effect(() => {
		if (tabbed && tab === 'preview' && paneBox !== null) {
			const box = paneBox;
			requestAnimationFrame(() => {
				if (box.offsetParent !== null) height = Math.max(384, Math.round(window.innerHeight - (box.getBoundingClientRect().top + window.scrollY) - 16));
			});
		}
	});

	function onkeydown(event: KeyboardEvent) {
		const moves: Record<string, number> = { ArrowLeft: share - STEP, ArrowRight: share + STEP, Home: MIN, End: MAX };
		const next = moves[event.key];
		if (next === undefined) return;
		event.preventDefault();
		setShare(next);
	}

	function onpointerdown(event: PointerEvent) {
		if (event.button !== 0 || container === null) return;
		const handle = event.currentTarget as HTMLElement;
		handle.setPointerCapture(event.pointerId);
		handle.focus();
		dragging = true;
		const box = container.getBoundingClientRect();
		const move = (moved: PointerEvent) => setShare(((moved.clientX - box.left) / box.width) * 100);
		const stop = () => {
			dragging = false;
			handle.removeEventListener('pointermove', move);
			handle.removeEventListener('pointerup', stop);
			handle.removeEventListener('pointercancel', stop);
		};
		handle.addEventListener('pointermove', move);
		handle.addEventListener('pointerup', stop);
		handle.addEventListener('pointercancel', stop);
	}

	function ontabkey(event: KeyboardEvent) {
		if (event.key !== 'ArrowLeft' && event.key !== 'ArrowRight' && event.key !== 'Home' && event.key !== 'End') return;
		event.preventDefault();
		tab = event.key === 'Home' ? 'edit' : event.key === 'End' ? 'preview' : tab === 'edit' ? 'preview' : 'edit';
		document.getElementById(`${id}-tab-${tab}`)?.focus();
	}

	const TAB = 'rounded-md px-3 py-1.5 text-sm font-medium text-muted-foreground transition-colors outline-none hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/40 aria-selected:bg-background aria-selected:text-foreground aria-selected:shadow-xs';
</script>

<div
	bind:this={container}
	class={split ? 'grid items-start' : ''}
	style:grid-template-columns={split ? `minmax(0, ${share}fr) 1rem minmax(0, ${100 - share}fr)` : undefined}
	data-testid="preview-split"
	data-layout={split ? 'side-by-side' : tabbed ? 'tabs' : 'closed'}
>
	{#if tabbed}
		<div role="tablist" aria-label="Edit or preview" class="mb-5 grid grid-cols-2 rounded-lg bg-muted p-1" data-testid="preview-tabs">
			<button role="tab" id="{id}-tab-edit" aria-selected={tab === 'edit'} aria-controls="{id}-editor" tabindex={tab === 'edit' ? 0 : -1} class={TAB} onclick={() => (tab = 'edit')} onkeydown={ontabkey}>Edit</button>
			<button role="tab" id="{id}-tab-preview" aria-selected={tab === 'preview'} aria-controls="{id}-preview" tabindex={tab === 'preview' ? 0 : -1} class={TAB} onclick={() => (tab = 'preview')} onkeydown={ontabkey}
				>Preview</button
			>
		</div>
	{/if}
	<div
		id="{id}-editor"
		class={split ? 'min-w-0 pr-5 sm:pr-8 lg:pr-10' : ''}
		role={tabbed ? 'tabpanel' : undefined}
		aria-labelledby={tabbed ? `${id}-tab-edit` : undefined}
		hidden={tabbed && tab === 'preview'}
	>
		{@render children()}
	</div>
	{#if open}
		{#if split}
			<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -- a focusable separator is the ARIA pattern for a resize handle -->
			<div
				role="separator"
				tabindex="0"
				aria-orientation="vertical"
				aria-label="Resize the editor and the preview"
				aria-controls="{id}-editor"
				aria-valuemin={MIN}
				aria-valuemax={MAX}
				aria-valuenow={share}
				aria-valuetext="Editor {share}%, preview {100 - share}%"
				title="Drag, or use the arrow keys, to resize"
				style:height={height === null ? undefined : `${height}px`}
				class="group sticky top-[4.5rem] flex h-[calc(100svh-6rem)] cursor-col-resize touch-none items-center justify-center self-start rounded-md outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
				{onkeydown}
				{onpointerdown}
				data-testid="preview-separator"
			>
				<span class="h-full w-px bg-border transition-colors group-hover:bg-ring group-focus-visible:bg-ring" aria-hidden="true"></span>
				<span
					class="absolute h-10 w-1.5 rounded-full border border-border bg-card shadow-xs transition-colors group-hover:border-ring group-focus-visible:border-ring {dragging ? 'border-ring' : ''}"
					aria-hidden="true"
				></span>
			</div>
		{/if}
		<div
			bind:this={paneBox}
			id="{id}-preview"
			style:height={height === null ? undefined : `${height}px`}
			class="min-w-0 {split ? 'sticky top-[4.5rem] h-[calc(100svh-6rem)] self-start' : 'h-[calc(100svh-10rem)] min-h-96'} {dragging ? '[&_iframe]:pointer-events-none' : ''}"
			role={tabbed ? 'tabpanel' : undefined}
			aria-labelledby={tabbed ? `${id}-tab-preview` : undefined}
			hidden={tabbed && tab === 'edit'}
		>
			{@render pane()}
		</div>
	{/if}
</div>
