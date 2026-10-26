<!--
  Light, dark or the system's choice. The new theme spreads from the button as a circle (a view
  transition) unless motion is reduced. Contrast follows the system's "more contrast" setting by
  itself (prefers-contrast), so there is nothing to choose for it.
-->
<script lang="ts">
	import MonitorIcon from '@lucide/svelte/icons/monitor';
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SunIcon from '@lucide/svelte/icons/sun';
	import { tick } from 'svelte';
	import { setMode, userPrefersMode } from 'mode-watcher';
	import { buttonVariants } from '@colophon/ui/ui/button';
	import * as DropdownMenu from '@colophon/ui/ui/dropdown-menu';

	const CHOICES = [
		{ value: 'light', label: 'Light', icon: SunIcon },
		{ value: 'dark', label: 'Dark', icon: MoonIcon },
		{ value: 'system', label: 'System', icon: MonitorIcon }
	] as const;
	let open = $state(false);
	let trigger = $state<HTMLElement | null>(null);

	function pick(value: string) {
		open = false;
		const next = value as 'light' | 'dark' | 'system';
		const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		if (!('startViewTransition' in document) || reduced) {
			setMode(next);
			return;
		}
		const box = trigger?.getBoundingClientRect();
		const x = box ? box.left + box.width / 2 : window.innerWidth - 32;
		const y = box ? box.top + box.height / 2 : 32;
		const radius = Math.hypot(Math.max(x, window.innerWidth - x), Math.max(y, window.innerHeight - y));
		const transition = document.startViewTransition(async () => {
			setMode(next);
			await tick();
		});
		void transition.ready.then(() => {
			document.documentElement.animate(
				{ clipPath: [`circle(0px at ${x}px ${y}px)`, `circle(${radius}px at ${x}px ${y}px)`] },
				{ duration: 480, easing: 'cubic-bezier(0.2, 0.8, 0.2, 1)', pseudoElement: '::view-transition-new(root)' }
			);
		});
	}
</script>

<DropdownMenu.Root bind:open>
	<DropdownMenu.Trigger bind:ref={trigger} class="{buttonVariants({ variant: 'ghost', size: 'icon-sm' })} relative" aria-label="Appearance">
		<SunIcon class="theme-icon rotate-0 scale-100 dark:-rotate-90 dark:scale-0" />
		<MoonIcon class="theme-icon absolute rotate-90 scale-0 dark:rotate-0 dark:scale-100" />
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
