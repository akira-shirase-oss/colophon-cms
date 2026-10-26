<!--
  Light, dark or the system's choice. Contrast follows the system's "more contrast" setting by
  itself (prefers-contrast), so there is nothing to choose for it.
-->
<script lang="ts">
	import MonitorIcon from '@lucide/svelte/icons/monitor';
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SunIcon from '@lucide/svelte/icons/sun';
	import { setMode, userPrefersMode } from 'mode-watcher';
	import { buttonVariants } from '@colophon/ui/ui/button';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';

	const CHOICES = [
		{ value: 'light', label: 'Light', icon: SunIcon },
		{ value: 'dark', label: 'Dark', icon: MoonIcon },
		{ value: 'system', label: 'System', icon: MonitorIcon }
	] as const;
	let open = $state(false);

	function pick(value: string) {
		setMode(value as 'light' | 'dark' | 'system');
		open = false;
	}
</script>

<DropdownMenu.Root bind:open>
	<DropdownMenu.Trigger class={buttonVariants({ variant: 'ghost', size: 'icon-sm' })} aria-label="Appearance">
		<SunIcon class="dark:hidden" />
		<MoonIcon class="hidden dark:block" />
	</DropdownMenu.Trigger>
	<DropdownMenu.Content align="end" class="w-40">
		<DropdownMenu.RadioGroup value={userPrefersMode.current} onValueChange={pick}>
			{#each CHOICES as choice (choice.value)}
				{@const Icon = choice.icon}
				<DropdownMenu.RadioItem value={choice.value}><Icon class="text-muted-foreground" />{choice.label}</DropdownMenu.RadioItem>
			{/each}
		</DropdownMenu.RadioGroup>
	</DropdownMenu.Content>
</DropdownMenu.Root>
