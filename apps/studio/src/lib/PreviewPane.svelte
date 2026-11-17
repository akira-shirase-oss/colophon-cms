<!--
  Preview on your site (D-181): the editor's draft on the real site, in a frame beside
  the fields. Opening issues a short-lived grant for this entry and loads its address; the site's
  preview route trades the one-time code for a token and says, by message, whether it is showing
  the draft. Each save the server acknowledged is told to the site (never a keystroke), the grant
  renews itself while the pane is open, and closing (or leaving) revokes it.

  Every state says what is happening in plain words: opening, live, a site that didn't confirm in
  10 seconds, a cookie the browser kept out of the frame, changes not saved yet, a save that failed,
  a preview that ended, and a site that couldn't be reached.
-->
<script lang="ts">
	import { onMount, untrack } from 'svelte';
	import { resolve } from '$app/paths';
	import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
	import CloudCheckIcon from '@lucide/svelte/icons/cloud-check';
	import CookieIcon from '@lucide/svelte/icons/cookie';
	import ExternalLinkIcon from '@lucide/svelte/icons/external-link';
	import HourglassIcon from '@lucide/svelte/icons/hourglass';
	import LoaderIcon from '@lucide/svelte/icons/loader-circle';
	import RotateCwIcon from '@lucide/svelte/icons/rotate-cw';
	import SettingsIcon from '@lucide/svelte/icons/settings-2';
	import XIcon from '@lucide/svelte/icons/x';
	import { toast } from 'svelte-sonner';
	import { Button } from '@colophon/ui/ui/button';
	import { Skeleton } from '@colophon/ui/ui/skeleton';
	import { ApiError } from './api.ts';
	import type { SaveStatus } from './saver.svelte.ts';
	import {
		draftSaved,
		endsIn,
		expired,
		grantOf,
		hostOf,
		issuePreview,
		openInNewTab,
		type PaneEvent,
		type PaneState,
		previewCode,
		previewProblem,
		QUIET_MS,
		readyFrom,
		RENEW_EVERY_MS,
		renewDue,
		renewPreview,
		revokePreview,
		revokePreviewOnExit,
		scaleFor,
		step,
		type Width,
		WIDTHS
	} from './preview.ts';
	import TechnicalDetails from './shell/TechnicalDetails.svelte';
	import { shownTitle } from './words.ts';

	let {
		project,
		entry,
		title,
		status,
		saved,
		manages,
		onclose
	}: {
		project: string;
		entry: string;
		/** The entry's title, to name the frame. */
		title: string;
		/** The autosave's state: what the preview is behind by, if anything. */
		status: SaveStatus;
		/** The last save the server acknowledged; each new one is told to the site. */
		saved: { revision: string; at: number } | null;
		/** Whether this role may set the preview address (`destinations.manage`). */
		manages: boolean;
		onclose: () => void;
	} = $props();

	let pane = $state.raw<PaneState>({ kind: 'closed' });
	const grant = $derived(grantOf(pane));
	function dispatch(event: PaneEvent) {
		pane = step(pane, event);
	}

	let frame = $state<HTMLIFrameElement | null>(null);
	let section = $state<HTMLElement | null>(null);
	/** What the frame shows: the grant's address, or a fresh code's. */
	let src = $state<string | null>(null);
	let expiresAt = $state<string | null>(null);
	let now = $state(Date.now());
	/** A save was told to the site since the pane opened. */
	let told = $state(false);
	let width = $state<Width>('fit');
	let room = $state(0);
	let roomHeight = $state(0);

	let quiet: ReturnType<typeof setTimeout> | undefined;
	let renewedAt = 0;
	let renewing = false;
	/** Each issue or reload; an answer for an older one is dropped (and its grant revoked). */
	let attempt = 0;
	let gone = false;

	const host = $derived(grant === null ? '' : hostOf(grant.url));
	const preset = $derived(WIDTHS.find((option) => option.value === width) ?? WIDTHS[3]);
	// A preset wider than the pane is shown smaller, never cut off or scrolled sideways.
	const gutter = $derived(preset.px === null ? 0 : 24);
	const scale = $derived(scaleFor(preset.px, room - gutter));

	function listen(url: string) {
		src = url;
		clearTimeout(quiet);
		quiet = setTimeout(() => dispatch({ type: 'quiet' }), QUIET_MS);
	}

	async function open() {
		dispatch({ type: 'open' });
		if (pane.kind !== 'opening' || pane.grant !== null) return;
		const mine = ++attempt;
		told = false;
		src = null;
		try {
			const issued = await issuePreview(project, entry);
			if (gone || mine !== attempt) {
				void revokePreview(project, issued.id).catch(() => {});
				return;
			}
			dispatch({ type: 'issued', grant: issued });
			expiresAt = issued.expires_at;
			renewedAt = Date.now();
			now = renewedAt;
			listen(issued.url);
		} catch (error) {
			if (gone || mine !== attempt) return;
			dispatch({ type: 'failed', error });
		}
	}

	/** Reloads through the site's route with a fresh code, keeping the grant. */
	async function reload() {
		const current = grantOf(pane);
		if (current === null) return;
		dispatch({ type: 'reload' });
		const mine = ++attempt;
		try {
			const fresh = await previewCode(project, current.id);
			if (gone || mine !== attempt) return;
			listen(fresh.url);
		} catch (error) {
			if (gone || mine !== attempt) return;
			clearTimeout(quiet);
			dispatch({ type: 'failed', error });
			if (pane.kind === 'ended') src = null;
		}
	}

	function retry() {
		if (grantOf(pane) !== null) void reload();
		else void open();
	}

	function end() {
		clearTimeout(quiet);
		src = null;
		expiresAt = null;
		dispatch({ type: 'ended' });
	}

	/** Keeps the grant alive while the pane is open; says so when it has ended. */
	async function keepAlive() {
		now = Date.now();
		const current = grantOf(pane);
		if (current === null || expiresAt === null || renewing) return;
		if (expired(expiresAt, now)) {
			end();
			return;
		}
		if (!renewDue(expiresAt, renewedAt, now)) return;
		renewing = true;
		try {
			const renewed = await renewPreview(project, current.id);
			if (gone || grantOf(pane)?.id !== current.id) return;
			expiresAt = renewed.expires_at;
			renewedAt = Date.now();
		} catch (error) {
			if (error instanceof ApiError && error.failure === 'not-found') end();
			// Anything else: try again in a minute, until the end itself passes.
			else renewedAt = Date.now() - RENEW_EVERY_MS + 60_000;
		} finally {
			renewing = false;
		}
	}

	/** "Open in a new tab": a grant of its own, so this frame's session is left alone. */
	async function newTab() {
		const failure = await openInNewTab(() => issuePreview(project, entry));
		if (failure !== null) {
			const said = previewProblem(failure);
			toast.error(said.title, { description: said.text });
		}
	}

	// The site says whether it is showing the draft: only its origin, exactly, and only this frame.
	$effect(() => {
		const onMessage = (event: MessageEvent) => {
			const current = grantOf(pane);
			const target = frame?.contentWindow;
			if (current === null || target === null || target === undefined) return;
			const previewing = readyFrom(event, current.origin, target);
			if (previewing === null) return;
			clearTimeout(quiet);
			dispatch({ type: 'ready', previewing });
		};
		window.addEventListener('message', onMessage);
		return () => window.removeEventListener('message', onMessage);
	});

	// Each acknowledged save, told to the site at its exact origin: it reads the new version.
	$effect(() => {
		const latest = saved;
		if (latest === null) return;
		untrack(() => {
			const current = grantOf(pane);
			const target = frame?.contentWindow;
			if (current === null || target === null || target === undefined || src === null) return;
			target.postMessage(draftSaved(entry, latest.revision), current.origin);
			told = true;
		});
	});

	onMount(() => {
		// Opened from the Preview menu: focus comes here, where the state is said.
		section?.focus({ preventScroll: true });
		void open();
		const timer = setInterval(() => void keepAlive(), 15_000);
		const onVisible = () => {
			if (document.visibilityState === 'visible') void keepAlive();
		};
		// Leaving the page (a reload, a closed tab) ends the grant on the way out.
		const onHide = () => {
			const current = grantOf(pane);
			if (current !== null) revokePreviewOnExit(project, current.id);
		};
		document.addEventListener('visibilitychange', onVisible);
		window.addEventListener('pagehide', onHide);
		return () => {
			gone = true;
			clearInterval(timer);
			clearTimeout(quiet);
			document.removeEventListener('visibilitychange', onVisible);
			window.removeEventListener('pagehide', onHide);
			const current = grantOf(pane);
			if (current !== null) void revokePreview(project, current.id).catch(() => {});
		};
	});

	/** The pill at the top: the state in a word or two, with a mark (never colour alone). */
	const pill = $derived.by(() => {
		switch (pane.kind) {
			case 'closed':
			case 'opening':
				return { text: 'Opening…', tone: 'neutral' } as const;
			case 'live':
				return { text: 'Live preview', tone: 'live' } as const;
			case 'cookie-blocked':
				return { text: 'Not showing your draft', tone: 'attention' } as const;
			case 'unconfirmed':
				return { text: 'Not confirmed', tone: 'pending' } as const;
			case 'ended':
				return { text: 'Ended', tone: 'neutral' } as const;
			case 'not-set-up':
				return { text: 'Not set up', tone: 'neutral' } as const;
			case 'failed':
				return { text: 'Didn’t open', tone: 'attention' } as const;
		}
	});
	const TONE = {
		neutral: 'bg-muted text-muted-foreground',
		live: 'bg-live-soft text-live-ink',
		pending: 'bg-pending-soft text-pending-ink',
		attention: 'bg-attention-soft text-attention-ink'
	} as const;
	const DOT = { neutral: 'bg-draft', live: 'bg-live', pending: 'bg-pending', attention: 'bg-attention' } as const;

	/** What the frame shows compared with the editor: only while a site is open. */
	const sync = $derived.by(() => {
		if (grant === null || pane.kind === 'opening') return null;
		switch (status) {
			case 'unsaved':
			case 'saving':
				return { text: 'Shows your last saved version', tone: 'pending' } as const;
			case 'saved':
				return told ? ({ text: 'Updated', tone: 'live' } as const) : ({ text: 'Shows your saved draft', tone: 'muted' } as const);
			default:
				return { text: 'Preview is behind: your latest changes aren’t saved', tone: 'attention' } as const;
		}
	});

	const problem = $derived(pane.kind === 'failed' ? previewProblem(pane.error) : null);
	const settingsHref = $derived(`${resolve('/projects/[project]/settings/connections', { project })}#preview`);
	const frameTitle = $derived(`Preview of ${shownTitle(title)}${host === '' ? '' : ` on ${host}`}`);
</script>

<section
	bind:this={section}
	tabindex="-1"
	class="flex h-full min-h-0 flex-col overflow-hidden rounded-xl border border-border bg-card shadow-xs outline-none focus-visible:ring-[3px] focus-visible:ring-ring/40"
	aria-label="Preview on your site"
	data-testid="preview-pane"
	data-state={pane.kind}
>
	<!-- The pane's own bar: where it stands, which site, until when; then what it shows and how wide.
	     A narrow pane gives the site's name a line of its own (a container query: the pane's width,
	     not the screen's). -->
	<header class="@container border-b border-border px-3 py-2">
		<div class="grid grid-cols-[auto_minmax(0,1fr)_auto] items-center gap-x-2 gap-y-1">
			<span class="col-start-1 row-start-1 inline-flex shrink-0 items-center gap-1.5 rounded-full px-2.5 py-0.5 text-xs font-medium {TONE[pill.tone]}" role="status" data-testid="preview-pill">
				{#if pane.kind === 'opening'}
					<LoaderIcon class="size-3 animate-spin" aria-hidden="true" />
				{:else}
					<span class="size-1.5 rounded-full {DOT[pill.tone]}" aria-hidden="true"></span>
				{/if}
				{pill.text}
			</span>
			<p class="col-span-3 col-start-1 row-start-2 min-h-4 min-w-0 truncate text-xs text-muted-foreground @md:col-span-1 @md:col-start-2 @md:row-start-1">
				{#if host !== ''}<span class="font-medium text-foreground" data-testid="preview-host">{host}</span>{/if}{#if host !== '' && expiresAt !== null}<span class="mx-1" aria-hidden="true">·</span><span
						title="Renews by itself while this stays open"
						data-testid="preview-ends">{endsIn(expiresAt, now)}</span
					>{/if}
			</p>
			<div class="col-start-3 row-start-1 flex shrink-0 items-center">
				<Button variant="ghost" size="icon-sm" aria-label="Reload the preview" title="Reload the preview" onclick={reload} disabled={grant === null} data-testid="preview-reload"
					><RotateCwIcon /></Button
				>
				<Button variant="ghost" size="icon-sm" aria-label="Open the preview in a new tab" title="Open in a new tab" onclick={newTab} disabled={pane.kind === 'not-set-up'} data-testid="preview-new-tab"
					><ExternalLinkIcon /></Button
				>
				<Button variant="ghost" size="icon-sm" aria-label="Close the preview" title="Close the preview" onclick={onclose} data-testid="preview-close"><XIcon /></Button>
			</div>
			<div class="col-span-3 col-start-1 row-start-3 mt-0.5 flex min-h-8 flex-wrap items-center justify-between gap-x-3 gap-y-1.5 @md:row-start-2">
			<p
				role="status"
				class="flex min-w-0 items-center gap-1.5 text-xs {sync?.tone === 'attention' ? 'font-medium text-attention-ink' : sync?.tone === 'live' ? 'text-live-ink' : 'text-muted-foreground'}"
				data-testid="preview-sync"
			>
				{#if sync?.tone === 'live'}<CloudCheckIcon class="size-3.5 shrink-0" aria-hidden="true" />{:else if sync?.tone === 'pending'}<span
						class="mx-1 size-1.5 shrink-0 rounded-full bg-pending"
						aria-hidden="true"
					></span>{:else if sync?.tone === 'attention'}<CircleAlertIcon
						class="size-3.5 shrink-0"
						aria-hidden="true"
					/>{/if}{sync?.text ?? ''}
			</p>
			<!-- The frame's width: native radios (Tab reaches the chosen one; arrow keys choose). -->
			<fieldset class="flex shrink-0 items-center gap-0.5 rounded-full border border-border bg-card p-0.5 shadow-soft" data-testid="preview-widths">
				<legend class="sr-only">Preview width</legend>
				{#each WIDTHS as option (option.value)}
					<label
						class="inline-flex h-7 min-w-9 cursor-pointer items-center justify-center rounded-full px-2 text-xs font-medium text-muted-foreground tabular transition-colors hover:text-foreground has-checked:bg-secondary has-checked:text-foreground has-focus-visible:ring-[3px] has-focus-visible:ring-ring/40"
						title={option.px === null ? 'Fit the pane' : `${option.name}, ${option.px} px`}
					>
						<input
							type="radio"
							class="sr-only"
							name="preview-width-{entry}"
							value={option.value}
							bind:group={width}
							aria-label={option.px === null ? 'Fit the pane' : `${option.name}, ${option.px} pixels wide`}
						/>{option.px === null ? 'Fit' : option.px}
					</label>
				{/each}
			</fieldset>
			</div>
		</div>
	</header>

	{#if pane.kind === 'unconfirmed'}
		<div class="border-b border-border bg-pending-soft/70 px-3 py-2.5 text-xs text-pending-ink" data-testid="preview-unconfirmed">
			<p class="font-semibold">We can’t tell whether your site loaded the preview</p>
			<p class="mt-0.5">It didn’t say it was showing your draft within 10 seconds. It may still be loading, or its preview route may not be set up yet.</p>
			<div class="mt-2 flex flex-wrap items-center gap-2">
				<Button variant="outline" size="xs" onclick={newTab}><ExternalLinkIcon data-icon="inline-start" />Open in a new tab</Button>
				{#if manages}
					<Button variant="outline" size="xs" href={settingsHref}><SettingsIcon data-icon="inline-start" />Check the address</Button>
				{:else}
					<span>Or ask an owner to check the preview address.</span>
				{/if}
			</div>
			<TechnicalDetails class="mt-2">
				<p>No <span class="font-mono">colophon:preview-ready</span> message from <span class="font-mono break-all">{grant?.origin}</span> within {QUIET_MS / 1000} s.</p>
				<p>The site’s preview route should use the SDK’s preview helper, which sends it once the page has loaded.</p>
			</TechnicalDetails>
		</div>
	{:else if pane.kind === 'cookie-blocked'}
		<div class="border-b border-border bg-attention-soft px-3 py-2.5 text-xs text-attention-ink" data-testid="preview-cookie-blocked">
			<p class="flex items-center gap-1.5 font-semibold"><CookieIcon class="size-3.5 shrink-0" aria-hidden="true" />Your browser kept the preview cookie out of this frame</p>
			<p class="mt-0.5">So your site shows what readers see, not your draft. A new tab works: there the site is the page you’re on.</p>
			<div class="mt-2"><Button variant="outline" size="xs" onclick={newTab}><ExternalLinkIcon data-icon="inline-start" />Open in a new tab</Button></div>
			<TechnicalDetails class="mt-2">
				<p>The site answered <span class="font-mono">colophon:preview-ready</span> with <span class="font-mono">previewing: false</span>: its partitioned cookie didn’t stick inside the frame, which happens when the browser blocks third-party cookies.</p>
			</TechnicalDetails>
		</div>
	{/if}

	<!-- The frame's room: always the same size, whatever state, so nothing jumps. -->
	<div class="relative min-h-0 flex-1 overflow-hidden bg-muted/60" bind:clientWidth={room} bind:clientHeight={roomHeight} data-testid="preview-room">
		{#if src !== null}
			<div class="mx-auto h-full {preset.px === null ? '' : 'py-3'}" style:width={preset.px === null ? '100%' : `${Math.round(preset.px * scale)}px`}>
				<iframe
					bind:this={frame}
					{src}
					title={frameTitle}
					sandbox="allow-scripts allow-same-origin allow-forms allow-popups"
					referrerpolicy="no-referrer"
					class="block origin-top-left border-0 bg-white {preset.px === null ? '' : 'rounded-md shadow-soft'}"
					class:invisible={pane.kind === 'opening'}
					style:width={preset.px === null ? '100%' : `${preset.px}px`}
					style:height={preset.px === null ? '100%' : `${Math.max(0, (roomHeight - gutter) / scale)}px`}
					style:transform={scale === 1 ? undefined : `scale(${scale})`}
					data-testid="preview-frame"
				></iframe>
			</div>
		{/if}
		{#if preset.px !== null && scale < 1 && src !== null && pane.kind !== 'opening'}
			<p class="absolute right-2 bottom-2 rounded-full bg-card/95 px-2 py-0.5 text-xs text-muted-foreground shadow-xs tabular" data-testid="preview-scale">
				{preset.px} px shown at {Math.round(scale * 100)}%
			</p>
		{/if}

		{#if pane.kind === 'opening' || pane.kind === 'closed'}
			<div class="absolute inset-0 flex flex-col bg-card" data-testid="preview-opening">
				<div class="space-y-4 p-5" aria-hidden="true">
					<div class="flex items-center justify-between"><Skeleton class="h-4 w-24 rounded-full" /><Skeleton class="h-4 w-40 rounded-full" /></div>
					<Skeleton class="h-36 w-full rounded-lg" />
					<Skeleton class="h-5 w-2/3 rounded-full" />
					<Skeleton class="h-3 w-full rounded-full" />
					<Skeleton class="h-3 w-5/6 rounded-full" />
					<Skeleton class="h-3 w-4/6 rounded-full" />
				</div>
				<p class="mt-auto flex items-center justify-center gap-2 p-5 text-sm text-muted-foreground">
					<LoaderIcon class="size-4 animate-spin" aria-hidden="true" />Opening a private preview of your site…
				</p>
			</div>
		{:else if pane.kind === 'ended'}
			<div class="absolute inset-0 flex items-center justify-center bg-card p-6" data-testid="preview-ended">
				<div class="max-w-sm text-center">
					<HourglassIcon class="mx-auto mb-3 size-6 text-muted-foreground" aria-hidden="true" />
					<h2 class="font-semibold">This preview ended</h2>
					<p class="mt-1 text-sm text-muted-foreground">Previews last a short while and end when they’re closed, so their link can’t be passed around. Start again to keep looking.</p>
					<Button class="mt-4" size="sm" onclick={open} data-testid="preview-start-again">Start again</Button>
				</div>
			</div>
		{:else if pane.kind === 'not-set-up'}
			<div class="absolute inset-0 flex items-center justify-center bg-card p-6" data-testid="preview-not-set-up">
				<div class="max-w-sm text-center">
					<h2 class="font-semibold">Preview on your site isn’t set up</h2>
					{#if manages}
						<p class="mt-1 text-sm text-muted-foreground">Add your site’s preview address in Settings, under Website connections.</p>
						<Button class="mt-4" size="sm" href={settingsHref}>Set it up</Button>
					{:else}
						<p class="mt-1 text-sm text-muted-foreground">Ask an owner of this project to add your site’s preview address.</p>
					{/if}
				</div>
			</div>
		{:else if pane.kind === 'failed' && problem !== null}
			<div class="absolute inset-0 flex items-center justify-center bg-card p-6" role="alert" data-testid="preview-failed">
				<div class="max-w-sm text-center">
					<CircleAlertIcon class="mx-auto mb-3 size-6 text-attention" aria-hidden="true" />
					<h2 class="font-semibold">{problem.title}</h2>
					<p class="mt-1 text-sm text-muted-foreground">{problem.text}</p>
					<Button class="mt-4" size="sm" onclick={retry} data-testid="preview-retry">Try again</Button>
					<TechnicalDetails class="mt-3 text-left"><p class="break-words">{problem.technical}</p></TechnicalDetails>
				</div>
			</div>
		{/if}
	</div>
</section>
