<!-- The Colophon mark (docs/brand/colophon-mark.svg): an octagonal C in the current text colour,
     so it is near-black on paper and near-white in the dark. Decorative unless labelled. -->
<script lang="ts">
	let { size = 22, label = '' }: { size?: number; label?: string } = $props();
</script>

<svg
	viewBox="0 0 292 310"
	width={size}
	height={Math.round(size * 1.06)}
	class="shrink-0"
	role={label ? 'img' : undefined}
	aria-label={label || undefined}
	aria-hidden={label ? undefined : 'true'}
	focusable="false"
>
	<path fill="currentColor" d="M87 0H232L292 60L232 120L194 82H130L87 125V185L130 228H194L232 190L292 250L232 310H87L0 223V87Z" />
</svg>
