<!--
  The Media page's upload queue (D2): files picked or dropped wait their turn and go
  three at a time (`CONCURRENCY`); each then waits while Colophon checks it. One shared poll
  — the library's still-checking uploads, one request every 1.5 s however many files wait — says
  when each settles, and only a settled one is read on its own. Every file ends in words: Ready,
  Rejected with the reason the checker gave, or Couldn't upload with what went wrong and Try
  again. A file still unchecked after two minutes is said to be slow, not failed: it shows in the
  library when it settles. At most `MAX_QUEUE` files wait at once; more are left out with a note.
-->
<script lang="ts">
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
	import ClockIcon from '@lucide/svelte/icons/clock';
	import RotateCcwIcon from '@lucide/svelte/icons/rotate-ccw';
	import { useQueryClient } from '@tanstack/svelte-query';
	import { onDestroy } from 'svelte';
	import { Button } from '@colophon/ui/ui/button';
	import { Progress } from '@colophon/ui/ui/progress';
	import { Spinner } from '@colophon/ui/ui/spinner';
	import { ApiError, type Asset, getAsset, listAssetPage, uploadAsset } from './api.ts';
	import { plainError } from './errors.ts';
	import { CONCURRENCY, MAX_QUEUE, sentence, size, unsendable } from './library.ts';

	let { project }: { project: string } = $props();

	type Status = 'waiting' | 'uploading' | 'checking' | 'slow' | 'ready' | 'rejected' | 'failed';
	interface Item {
		key: number;
		file: File;
		status: Status;
		/** Why it was rejected or couldn't upload, in plain words. */
		reason: string;
		/** The upload's id once the server accepted it. */
		asset: string | null;
		/** When the server accepted it, for "slow". */
		since: number;
		/** Whether sending it again can help (no answer, or the server was busy). */
		retryable: boolean;
	}

	/** After this long unchecked, a file is said to be slow (not failed). */
	const SLOW_MS = 120_000;
	/** After this long, the queue stops asking; the library still shows it when it settles. */
	const GIVE_UP_MS = 600_000;
	const POLL_MS = 1_500;

	const queryClient = useQueryClient();
	let items = $state<Item[]>([]);
	let note = $state('');
	let next = 0;
	let running = 0;

	const done = $derived(items.filter((item) => item.status === 'ready' || item.status === 'rejected' || item.status === 'failed').length);
	const busy = $derived(items.some((item) => item.status === 'waiting' || item.status === 'uploading' || item.status === 'checking'));
	const sending = $derived(items.some((item) => item.status === 'waiting' || item.status === 'uploading'));
	const tally = $derived({
		ready: items.filter((item) => item.status === 'ready').length,
		rejected: items.filter((item) => item.status === 'rejected').length,
		failed: items.filter((item) => item.status === 'failed').length,
		slow: items.filter((item) => item.status === 'slow').length
	});
	const summary = $derived.by(() => {
		if (busy) return `Uploading ${items.length === 1 ? '1 file' : `${items.length} files`}: ${done} of ${items.length} done`;
		const parts = [
			tally.ready > 0 ? `${tally.ready} ready` : '',
			tally.rejected > 0 ? `${tally.rejected} rejected` : '',
			tally.failed > 0 ? `${tally.failed} couldn't upload` : '',
			tally.slow > 0 ? `${tally.slow} still being checked` : ''
		].filter((part) => part !== '');
		return `Uploads finished: ${parts.join(', ')}`;
	});

	/** Queues `files` (a pick or a drop): at most `MAX_QUEUE` waiting at once. */
	export function add(files: Iterable<File>) {
		const list = Array.from(files);
		if (list.length === 0) return;
		const waiting = items.filter((item) => item.status === 'waiting' || item.status === 'uploading').length;
		const room = Math.max(0, MAX_QUEUE - waiting);
		const taken = list.slice(0, room);
		const left = list.length - taken.length;
		note = left > 0 ? `Only ${MAX_QUEUE} files can wait at once, so ${left === 1 ? '1 file was' : `${left} files were`} left out. Add ${left === 1 ? 'it' : 'them'} again when these finish.` : '';
		for (const file of taken) {
			// What the server would refuse anyway is said at once, and never sent.
			const refused = unsendable(file);
			items.push({ key: (next += 1), file, status: refused === null ? 'waiting' : 'failed', reason: refused ?? '', asset: null, since: 0, retryable: false });
		}
		pump();
	}

	/** Starts waiting files while fewer than `CONCURRENCY` are on their way. */
	function pump() {
		while (running < CONCURRENCY) {
			const item = items.find((candidate) => candidate.status === 'waiting');
			if (item === undefined) return;
			running += 1;
			item.status = 'uploading';
			void send(item).finally(() => {
				running -= 1;
				pump();
			});
		}
	}

	async function send(item: Item) {
		try {
			const asset = await uploadAsset(project, item.file);
			item.asset = asset.id;
			item.since = Date.now();
			settle(item, asset);
			refresh();
		} catch (error) {
			item.status = 'failed';
			item.retryable = !(error instanceof ApiError) || error.failure === 'retry';
			item.reason = plainError(error, `upload “${item.file.name}”`);
		}
	}

	/** Moves `item` on from what the server says about its upload. */
	function settle(item: Item, asset: Asset) {
		switch (asset.state) {
			case 'usable':
				item.status = 'ready';
				break;
			case 'rejected':
				item.status = 'rejected';
				item.reason = sentence(asset.rejected_reason ?? "Colophon couldn't read this file as an image.");
				break;
			case 'quarantined':
				item.status = Date.now() - item.since > SLOW_MS ? 'slow' : 'checking';
				watch();
				break;
			default:
				item.status = 'failed';
				item.reason = 'It was deleted before it finished.';
		}
	}

	// One poll for every file being checked.
	let timer: ReturnType<typeof setInterval> | undefined;
	let asking = false;
	function watch() {
		timer ??= setInterval(() => void tick(), POLL_MS);
	}
	async function tick() {
		const waiting = items.filter((item) => (item.status === 'checking' || item.status === 'slow') && item.asset !== null && Date.now() - item.since < GIVE_UP_MS);
		if (waiting.length === 0) {
			clearInterval(timer);
			timer = undefined;
			return;
		}
		if (asking) return;
		asking = true;
		try {
			const unchecked = (await listAssetPage(project, { state: 'quarantined', limit: 100 })).data;
			let moved = false;
			for (const item of waiting) {
				const listed = unchecked.find((asset) => asset.id === item.asset);
				const asset = listed ?? (await getAsset(project, item.asset ?? ''));
				const before = item.status;
				settle(item, asset);
				moved ||= item.status !== before && item.status !== 'slow';
			}
			if (moved) refresh();
		} catch {
			// A missed poll is asked again on the next tick; the library's own refresh still runs.
		} finally {
			asking = false;
		}
	}

	// The library refreshes once after a burst of changes, not once per file.
	let refreshing: ReturnType<typeof setTimeout> | undefined;
	function refresh() {
		clearTimeout(refreshing);
		refreshing = setTimeout(() => void queryClient.invalidateQueries({ queryKey: ['assets', project] }), 500);
	}

	function retry(item: Item) {
		item.status = 'waiting';
		item.reason = '';
		pump();
	}

	function clear() {
		items = items.filter((item) => item.status === 'waiting' || item.status === 'uploading' || item.status === 'checking');
		note = '';
	}

	// Leaving mid-upload loses what hasn't been sent: the browser asks first.
	function guard(event: BeforeUnloadEvent) {
		if (sending) event.preventDefault();
	}

	onDestroy(() => {
		clearInterval(timer);
		clearTimeout(refreshing);
	});

	const words: Record<Status, string> = {
		waiting: 'Waiting',
		uploading: 'Uploading…',
		checking: 'Checking…',
		slow: 'Still being checked — it shows in the library when it’s done',
		ready: 'Ready',
		rejected: 'Rejected',
		failed: "Couldn't upload"
	};
</script>

<svelte:window onbeforeunload={guard} />

{#if items.length > 0 || note !== ''}
	<section class="mb-6 rounded-xl border border-border bg-card p-4" aria-label="Uploads" data-testid="upload-queue">
		<div class="flex flex-wrap items-center justify-between gap-2">
			<p class="text-sm font-medium" role="status" data-testid="upload-summary">{summary}</p>
			{#if done > 0 || tally.slow > 0}
				<Button variant="ghost" size="sm" onclick={clear} data-testid="upload-clear">Clear finished</Button>
			{/if}
		</div>
		{#if items.length > 0}
			<Progress class="mt-3" value={done} max={items.length} aria-label="Uploads done" />
		{/if}
		{#if note !== ''}
			<p class="mt-3 text-sm text-attention-ink" role="alert" data-testid="upload-note">{note}</p>
		{/if}
		<ul class="mt-3 grid max-h-72 gap-1 overflow-y-auto" aria-label="Files">
			{#each items as item (item.key)}
				<li class="flex min-w-0 items-start gap-3 rounded-lg px-2 py-1.5 text-sm" data-testid="upload-item" data-status={item.status}>
					<span class="mt-0.5 shrink-0" aria-hidden="true">
						{#if item.status === 'uploading' || item.status === 'checking'}
							<Spinner class="text-muted-foreground" role="presentation" aria-label="" />
						{:else if item.status === 'ready'}
							<CircleCheckIcon class="size-4 text-live-ink" />
						{:else if item.status === 'rejected' || item.status === 'failed'}
							<CircleAlertIcon class="size-4 text-attention-ink" />
						{:else}
							<ClockIcon class="size-4 text-muted-foreground" />
						{/if}
					</span>
					<div class="min-w-0 flex-1">
						<div class="flex min-w-0 flex-wrap items-baseline gap-x-2">
							<span class="min-w-0 truncate font-medium" title={item.file.name} data-user-content>{item.file.name}</span>
							<span class="text-xs text-muted-foreground">{size(item.file.size)}</span>
						</div>
						<p class="text-xs {item.status === 'rejected' || item.status === 'failed' ? 'text-attention-ink' : 'text-muted-foreground'}" data-testid="upload-status">
							{words[item.status]}{#if item.reason !== ''}: {item.reason}{/if}
						</p>
					</div>
					{#if item.status === 'failed' && item.retryable}
						<Button variant="outline" size="sm" class="shrink-0" onclick={() => retry(item)} aria-label="Try uploading {item.file.name} again"
							><RotateCcwIcon data-icon="inline-start" />Try again</Button
						>
					{/if}
				</li>
			{/each}
		</ul>
	</section>
{/if}
