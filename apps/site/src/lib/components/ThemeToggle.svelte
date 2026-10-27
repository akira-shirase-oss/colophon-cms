<!--
  One button that switches between light and dark. Until someone presses it, the site follows the
  system's choice; after that, the choice is remembered. The new theme spreads from the button as a
  circle, unless motion is reduced. Contrast follows the system's "more contrast" setting by itself,
  so there is nothing to choose for it.
-->
<script lang="ts">
	import MoonIcon from '@lucide/svelte/icons/moon';
	import SunIcon from '@lucide/svelte/icons/sun';
	import { tick } from 'svelte';
	import { mode, setMode } from 'mode-watcher';

	let button = $state<HTMLButtonElement | null>(null);
	const dark = $derived(mode.current === 'dark');

	function toggle() {
		const next = dark ? 'light' : 'dark';
		const reduced = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		if (!('startViewTransition' in document) || reduced) {
			setMode(next);
			return;
		}
		const box = button?.getBoundingClientRect();
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

<button
	bind:this={button}
	type="button"
	class="relative grid size-10 place-items-center rounded-full text-foreground outline-none hover:bg-[var(--tertiary-hover)] focus-visible:ring-2 focus-visible:ring-ring [&_svg]:size-[18px]"
	aria-label="Dark mode"
	aria-pressed={dark}
	title={dark ? 'Switch to light' : 'Switch to dark'}
	onclick={toggle}
>
	<SunIcon class="theme-icon scale-100 rotate-0 dark:scale-0 dark:-rotate-90" aria-hidden="true" />
	<MoonIcon class="theme-icon absolute scale-0 rotate-90 dark:scale-100 dark:rotate-0" aria-hidden="true" />
</button>
