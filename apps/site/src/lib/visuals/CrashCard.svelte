<!-- Crash-proof publishing (P1, P2): what the crash tests do to the real binary, in order. -->
<script lang="ts">
	import CheckIcon from '@lucide/svelte/icons/check';
	import ZapIcon from '@lucide/svelte/icons/zap';

	let { class: className = '' }: { class?: string } = $props();
	const LINES = [
		{ kill: true, text: 'Server killed right after a save commits' },
		{ kill: false, text: 'Restarted; the retry gets the same answer, stored once' },
		{ kill: true, text: 'Worker killed mid-announcement' },
		{ kill: false, text: 'Next attempt resends with the same key; never an older release' }
	];
</script>

<ul class="grid gap-2 {className}">
	{#each LINES as line (line.text)}
		<li class="flex items-start gap-2.5 rounded-xl border border-border bg-card px-3 py-2 text-[0.8125rem] shadow-hairline">
			{#if line.kill}
				<ZapIcon class="mt-0.5 size-4 shrink-0 text-attention-ink" aria-hidden="true" /><span class="sr-only">Fault:</span>
			{:else}
				<CheckIcon class="mt-0.5 size-4 shrink-0 text-live-ink" aria-hidden="true" /><span class="sr-only">Result:</span>
			{/if}
			<span>{line.text}</span>
		</li>
	{/each}
</ul>
