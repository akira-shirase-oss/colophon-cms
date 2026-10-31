<!--
  One click flips light to dark and back. The new theme spreads from the button as a circle (a
  view transition) unless motion is reduced. Contrast follows the system's "more contrast"
  setting by itself (prefers-contrast), so there is nothing to choose for it.
-->
<script lang="ts">
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SunIcon from '@lucide/svelte/icons/sun';
	import { tick } from 'svelte';
	import { toggleMode } from 'mode-watcher';
	import { Button } from '@colophon/ui/ui/button';

	let button = $state<HTMLElement | null>(null);

	function toggle() {
		const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		if (!('startViewTransition' in document) || reduced) {
			toggleMode();
			return;
		}
		const box = button?.getBoundingClientRect();
		const x = box ? box.left + box.width / 2 : window.innerWidth - 32;
		const y = box ? box.top + box.height / 2 : 32;
		const radius = Math.hypot(Math.max(x, window.innerWidth - x), Math.max(y, window.innerHeight - y));
		const transition = document.startViewTransition(async () => {
			toggleMode();
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

<Button
	bind:ref={button}
	variant="ghost"
	size="icon-sm"
	class="relative"
	onclick={toggle}
	aria-label="Switch between light and dark"
	title="Switch between light and dark"
>
	<SunIcon class="theme-icon rotate-0 scale-100 dark:-rotate-90 dark:scale-0" />
	<MoonIcon class="theme-icon absolute rotate-90 scale-0 dark:rotate-0 dark:scale-100" />
</Button>
