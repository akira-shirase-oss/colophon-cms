<script lang="ts">
	import mark from '#lib/colophon-mark.svg';

	let { size = 22 }: { size?: number } = $props();
</script>

<img src={mark} alt="" width={size} height={Math.round(size * 1.1)} class="shrink-0 dark:invert" />
