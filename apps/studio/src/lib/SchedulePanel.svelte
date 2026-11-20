<!--
  A publish request's schedule (D-060), as the request page's Schedule card. Pick a date, a time
  and a time zone, and it publishes then, as you, with every check a publish makes. A time the
  clocks show twice (they go back) or skip (they go forward) is never guessed: the form shows both
  moments it could mean and asks. The form opens from the request's "Schedule…"; once a schedule
  is pending, the page's header says when it publishes and offers Publish now and Cancel schedule,
  and this card says who set it and what it waits on. Past schedules say how they went, including
  when one started late, or was blocked and why.
-->
<script lang="ts">
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import { ApiError, type Changeset, type Schedule, scheduleChangeset } from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';
	import { ago, meQuery } from '#lib/queries.ts';

	let {
		project,
		changeset,
		canSchedule,
		form = $bindable(false)
	}: { project: string; changeset: Changeset; canSchedule: boolean; form?: boolean } = $props();

	const queryClient = useQueryClient();
	const mine = createQuery(() => schedulesQuery(project, changeset.id));
	const pending = $derived(mine.data?.find((item) => item.state === 'scheduled'));
	// A schedule is cancelled by whoever set it or by someone who may publish what it publishes
	// (the server's rule); everyone else is told who can.
	const who = createQuery(meQuery);
	const cancels = $derived(canSchedule || (pending !== undefined && who.data != null && pending.scheduled_by === who.data.id));
	// When the pending one runs, the changeset's status changes too: read it again.
	let ran: number | null = null;
	$effect(() => {
		const finished = (mine.data ?? []).filter((item) => item.state !== 'scheduled').length;
		if (ran !== null && finished > ran) void refresh();
		ran = finished;
	});
	const past = $derived((mine.data ?? []).filter((item) => item.state !== 'scheduled'));
	const showsForm = $derived(form && canSchedule && changeset.status.state === 'approved' && pending === undefined);

	const here = Intl.DateTimeFormat().resolvedOptions().timeZone;
	const ZONES: string[] = (() => {
		const supported = (Intl as unknown as { supportedValuesOf?: (key: string) => string[] }).supportedValuesOf?.('timeZone') ?? [];
		return supported.includes('UTC') ? supported : ['UTC', ...supported];
	})();

	/** Tomorrow at nine, here: a sensible first suggestion. */
	function tomorrow(): { date: string; time: string } {
		const at = new Date(Date.now() + 24 * 3600 * 1000);
		const pad = (value: number) => String(value).padStart(2, '0');
		return { date: `${at.getFullYear()}-${pad(at.getMonth() + 1)}-${pad(at.getDate())}`, time: '09:00' };
	}

	let date = $state(tomorrow().date);
	let time = $state(tomorrow().time);
	let zone = $state(here);
	let busy = $state(false);
	let error = $state('');
	/** When the time needs a choice: which kind, and the two moments it could mean. */
	let ask = $state<{ reason: 'ambiguous_local_time' | 'skipped_local_time'; candidates: string[] } | null>(null);
	let choice = $state<'earlier' | 'later'>('earlier');
	let dateInput = $state<HTMLInputElement | null>(null);
	// Opening the form puts the cursor in it, so Schedule… leads somewhere on any screen.
	$effect(() => {
		if (showsForm && dateInput !== null) {
			dateInput.scrollIntoView({ block: 'nearest' });
			dateInput.focus();
		}
	});

	async function refresh() {
		await Promise.all([
			queryClient.invalidateQueries({ queryKey: ['schedules', project] }),
			queryClient.invalidateQueries({ queryKey: ['changeset', project, changeset.id] }),
			queryClient.invalidateQueries({ queryKey: ['releases', project] })
		]);
	}

	async function schedule(event: SubmitEvent) {
		event.preventDefault();
		busy = true;
		error = '';
		try {
			await scheduleChangeset(project, changeset.id, changeset.hash, {
				local_time: `${date}T${time}`,
				time_zone: zone,
				...(ask === null ? {} : { choice })
			});
			ask = null;
			form = false;
			await refresh();
		} catch (failure) {
			const reason = failure instanceof ApiError ? failure.reason : null;
			const candidates = failure instanceof ApiError ? failure.details?.candidates : undefined;
			if ((reason === 'ambiguous_local_time' || reason === 'skipped_local_time') && candidates?.length === 2) {
				ask = { reason, candidates };
				choice = 'earlier';
			} else {
				error = plainError(failure, 'schedule this');
			}
		} finally {
			busy = false;
		}
	}

	function outcome(item: Schedule): string {
		switch (item.state) {
			case 'published':
				return item.delayed ? `Published late, as Release ${item.generation}` : `Published on time, as Release ${item.generation}`;
			case 'blocked':
				return `Not published: ${item.reason ?? 'refused'}`;
			case 'cancelled':
				return 'Cancelled';
			default:
				return 'Scheduled';
		}
	}
</script>

<script lang="ts" module>
	import { listSchedules as list } from '#lib/api.ts';

	/** One request's schedules: the page's header and this card read the same answer. A due
	 *  schedule runs on the worker; its commit arrives as a `schedules` hint and the live stream
	 *  refreshes this (D-108) — nothing to poll for. */
	export const schedulesQuery = (project: string, changeset: string) => ({
		queryKey: ['schedules', project, changeset],
		queryFn: () => list(project, changeset)
	});

	/** A moment as it reads in `timeZone`, with the zone's abbreviation: "Thursday, 19 November
	 *  2026 at 14:00:00 UTC". */
	export function reads(instant: string, timeZone: string): string {
		return new Intl.DateTimeFormat(undefined, { dateStyle: 'full', timeStyle: 'long', timeZone }).format(new Date(instant));
	}

	/** The same moment, short: "Thu 19 Nov, 14:00 UTC". */
	export function readsShort(instant: string, timeZone: string): string {
		return new Intl.DateTimeFormat(undefined, {
			weekday: 'short',
			day: 'numeric',
			month: 'short',
			hour: '2-digit',
			minute: '2-digit',
			timeZoneName: 'short',
			timeZone
		}).format(new Date(instant));
	}
</script>

{#if showsForm || pending || past.length > 0}
	<section class="rounded-xl border border-border bg-card p-4" aria-labelledby="schedule-heading" data-testid="schedule-panel">
		<h2 id="schedule-heading" class="flex items-center gap-2 text-base font-semibold"><CalendarClockIcon class="size-4" aria-hidden="true" />Schedule</h2>

		{#if pending}
			<div class="mt-2 space-y-2 text-sm text-muted-foreground" data-testid="schedule-pending">
				{#if pending.time_zone !== here}
					<p>That's {readsShort(pending.run_at, here)} where you are.</p>
				{/if}
				<p>
					Set by {pending.scheduled_by_name ?? 'someone'}, {ago(pending.created_at)}. It publishes as them, if their role still allows it and nothing in it
					has changed.
				</p>
				{#if !cancels}
					<p data-testid="cannot-cancel">Its author or someone who can publish it can cancel it.</p>
				{/if}
			</div>
		{:else if showsForm}
			<form class="mt-3 grid gap-3" onsubmit={schedule} aria-label="Schedule publishing">
				<p class="text-sm text-muted-foreground">Publish this exact version later, at a time you choose.</p>
				<div class="grid grid-cols-2 gap-2">
					<div class="grid gap-1.5">
						<Label for="schedule-date">Date</Label>
						<Input id="schedule-date" type="date" bind:value={date} bind:ref={dateInput} required />
					</div>
					<div class="grid gap-1.5">
						<Label for="schedule-time">Time</Label>
						<Input id="schedule-time" type="time" bind:value={time} required />
					</div>
				</div>
				<div class="grid gap-1.5">
					<Label for="schedule-zone">Time zone</Label>
					<select
						id="schedule-zone"
						class="h-9 w-full rounded-control border border-input bg-popover px-3 text-sm outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
						bind:value={zone}
						onchange={() => (ask = null)}
					>
						{#each ZONES as name (name)}<option value={name}>{name}{name === here ? ' (yours)' : ''}</option>{/each}
					</select>
				</div>
				{#if ask}
					<fieldset class="grid gap-2 rounded-lg bg-pending-soft p-3 text-sm text-pending-ink" data-testid="schedule-choice">
						<legend class="sr-only">Which moment</legend>
						<p>
							{#if ask.reason === 'ambiguous_local_time'}
								The clocks go back that night in {zone}, so {time} happens twice. Which one?
							{:else}
								The clocks go forward that night in {zone}, so {time} doesn't happen. Publish at:
							{/if}
						</p>
						{#each ['earlier', 'later'] as const as option, index (option)}
							<label class="flex items-center gap-2">
								<input type="radio" name="schedule-choice" value={option} bind:group={choice} />
								{reads(ask.candidates[index] ?? '', zone)}
							</label>
						{/each}
					</fieldset>
				{/if}
				{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
				<!-- Secondary: on an approved request, Publish now is the page's one primary. -->
				<div class="flex flex-wrap gap-2">
					<Button type="submit" variant="outline" disabled={busy} data-testid="schedule-submit">{ask ? 'Schedule at this moment' : 'Schedule'}</Button>
					<Button type="button" variant="ghost" disabled={busy} onclick={() => ((form = false), (ask = null), (error = ''))}>Not now</Button>
				</div>
			</form>
		{/if}

		{#if past.length > 0}
			<ul class="mt-4 space-y-2 border-t border-dotted border-border pt-3 text-sm" class:mt-2={!pending && !showsForm} class:border-t-0={!pending && !showsForm} class:pt-0={!pending && !showsForm} data-testid="schedule-history">
				{#each past as item (item.id)}
					<li>
						<span class="flex flex-wrap items-center gap-2">
							{outcome(item)}
							{#if item.state === 'published' && item.delayed}<Badge variant="attention">delayed</Badge>{/if}
						</span>
						<span class="block text-xs text-muted-foreground">Was for {reads(item.run_at, item.time_zone)}</span>
					</li>
				{/each}
			</ul>
		{/if}
	</section>
{/if}
