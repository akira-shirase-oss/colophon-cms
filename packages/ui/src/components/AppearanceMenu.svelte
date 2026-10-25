<!-- Light, dark or the system's, at standard, medium or high contrast (Bits UI DropdownMenu). -->
<script lang="ts">
	import { DropdownMenu } from 'bits-ui';
	import { onMount } from 'svelte';
	import { applyAppearance, type ContrastChoice, storedContrast, storedTheme, type ThemeChoice } from '../theme.ts';

	let theme = $state<ThemeChoice>('system');
	let contrast = $state<ContrastChoice>('system');

	onMount(() => {
		theme = storedTheme();
		contrast = storedContrast();
		applyAppearance(theme, contrast);
	});

	function choose(next: { theme?: ThemeChoice; contrast?: ContrastChoice }) {
		theme = next.theme ?? theme;
		contrast = next.contrast ?? contrast;
		applyAppearance(theme, contrast);
	}

	const themes: [ThemeChoice, string][] = [['system', 'System'], ['light', 'Light'], ['dark', 'Dark']];
	const contrasts: [ContrastChoice, string][] = [['system', 'System'], ['standard', 'Standard'], ['medium', 'Medium'], ['high', 'High']];
</script>

<DropdownMenu.Root>
	<DropdownMenu.Trigger class="appearance-trigger">Appearance</DropdownMenu.Trigger>
	<DropdownMenu.Portal>
		<DropdownMenu.Content class="menu" align="end" sideOffset={4}>
			<DropdownMenu.RadioGroup value={theme} onValueChange={(value) => choose({ theme: value as ThemeChoice })}>
				<DropdownMenu.GroupHeading class="menu-heading">Theme</DropdownMenu.GroupHeading>
				{#each themes as [value, label] (value)}
					<DropdownMenu.RadioItem {value} class="menu-item">
						{#snippet children({ checked })}<span class="check" aria-hidden="true">{checked ? '●' : ''}</span>{label}{/snippet}
					</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
			<DropdownMenu.Separator class="menu-separator" />
			<DropdownMenu.RadioGroup value={contrast} onValueChange={(value) => choose({ contrast: value as ContrastChoice })}>
				<DropdownMenu.GroupHeading class="menu-heading">Contrast</DropdownMenu.GroupHeading>
				{#each contrasts as [value, label] (value)}
					<DropdownMenu.RadioItem {value} class="menu-item">
						{#snippet children({ checked })}<span class="check" aria-hidden="true">{checked ? '●' : ''}</span>{label}{/snippet}
					</DropdownMenu.RadioItem>
				{/each}
			</DropdownMenu.RadioGroup>
		</DropdownMenu.Content>
	</DropdownMenu.Portal>
</DropdownMenu.Root>

<style>
	:global(.appearance-trigger) {
		height: 2.5rem;
		padding: 0 1rem;
		border: 1px solid var(--md-sys-color-outline);
		border-radius: var(--md-sys-shape-corner-full);
		background: transparent;
		color: var(--md-sys-color-primary);
		font: var(--md-sys-typescale-label-large);
		cursor: pointer;
	}
	:global(.menu) {
		min-width: 12rem;
		padding: 0.5rem 0;
		border-radius: var(--md-sys-shape-corner-extra-small);
		background: var(--md-sys-color-surface-container);
		color: var(--md-sys-color-on-surface);
		box-shadow: var(--md-sys-elevation-level2);
		z-index: 10;
	}
	:global(.menu-heading) {
		padding: 0.5rem 1rem 0.25rem;
		font: var(--md-sys-typescale-label-medium);
		color: var(--md-sys-color-on-surface-variant);
	}
	:global(.menu-item) {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		height: 2.5rem;
		padding: 0 1rem;
		font: var(--md-sys-typescale-label-large);
		cursor: pointer;
		outline: none;
	}
	:global(.menu-item[data-highlighted]) {
		background: color-mix(in srgb, var(--md-sys-color-on-surface) 8%, transparent);
	}
	:global(.menu-separator) {
		height: 1px;
		margin: 0.5rem 0;
		background: var(--md-sys-color-outline-variant);
	}
	.check {
		width: 1rem;
		color: var(--md-sys-color-primary);
	}
</style>
