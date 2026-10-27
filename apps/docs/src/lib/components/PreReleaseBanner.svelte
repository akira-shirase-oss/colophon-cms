<!--
  On every page, above everything else: Colophon is pre-release and not for production. A
  sentence, not a dismissible toast, so it is there for every reader of every page, including the
  ones who arrive from search.
-->
<script lang="ts">
	import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
	import { href } from '#lib/paths.ts';
</script>

<div class="banner px-4 py-2 text-center text-[0.8125rem] leading-5" role="note" aria-label="Pre-release" data-testid="pre-release-banner" data-pagefind-ignore>
	<TriangleAlertIcon class="mr-1 inline size-3.5 -translate-y-px" aria-hidden="true" />
	<strong class="font-semibold">Pre-release, not for production.</strong>
	Colophon v0.1 is not out; anything here can change.
	<a href={href('/resources/roadmap/')} class="font-medium underline underline-offset-2">Where it stands</a>
</div>

<style>
	.banner {
		background: var(--pending-soft);
		color: var(--pending-ink);
	}
	.banner a:focus-visible {
		outline: 2px solid var(--ring);
		outline-offset: 2px;
		border-radius: 0.25rem;
	}
</style>
