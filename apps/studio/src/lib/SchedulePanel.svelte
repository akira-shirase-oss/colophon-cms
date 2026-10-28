<!--
  Scheduling a changeset (D-060): pick a date, a time and a time zone, and it publishes then, as
  you, with every check a publish makes. A time the clocks show twice (they go back) or skip
  (they go forward) is never guessed: the panel shows both moments it could mean and asks. A
  pending schedule can be cancelled; past ones say how they went, including when one started
  late, or was blocked and why.
-->
<script lang="ts">
	import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
	import { createQuery, useQueryClient } from '@tanstack/svelte-query';
	import { Badge } from '@colophon/ui/ui/badge';
	import { Button } from '@colophon/ui/ui/button';
	import { Input } from '@colophon/ui/ui/input';
	import { Label } from '@colophon/ui/ui/label';
	import { ApiError, cancelSchedule, type Changeset, listSchedules, type Schedule, scheduleChangeset } from '#lib/api.ts';
	import { plainError } from '#lib/errors.ts';
	import { ago } from '#lib/queries.ts';

	let { project, changeset, canSchedule }: { project: string; changeset: Changeset; canSchedule: boolean } = $props();

	const queryClient = useQueryClient();
	const key = $derived(['schedules', project, changeset.id]);
	const mine = createQuery(() => ({
		queryKey: key,
		queryFn: () => listSchedules(project, changeset.id),
		// While one is due within a minute, look again until it has run.
		refetchInterval: (query) =>
			(query.state.data ?? []).some((item) => item.state === 'scheduled' && Date.parse(item.run_at) - Date.now() < 60_000) ? 2_000 : false
	}));
	const pending = $derived(mine.data?.find((item) => item.state === 'scheduled'));
	// When the pending one runs, the changeset's status changes too: read it again.
	let ran: number | null = null;
	$effect(() => {
		const finished = (mine.data ?? []).filter((item) => item.state !== 'scheduled').length;
		if (ran !== null && finished > ran) void refresh();
		ran = finished;
	});
	const past = $derived((mine.data ?? []).filter((item) => item.state !== 'scheduled'));

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

	/** A moment as it reads in `timeZone`, with the zone's abbreviation. */
	function reads(instant: string, timeZone: string): string {
		return new Intl.DateTimeFormat(undefined, { dateStyle: 'full', timeStyle: 'long', timeZone }).format(new Date(instant));
	}

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

	async function cancel(item: Schedule) {
		busy = true;
		error = '';
		try {
			await cancelSchedule(project, item.id);
		} catch (failure) {
			error = plainError(failure, 'cancel this schedule');
		} finally {
			busy = false;
			await refresh();
		}
	}

	function outcome(item: Schedule): string {
		switch (item.state) {
			case 'published':
				return item.delayed ? `Published late, as release ${item.generation}` : `Published on time, as release ${item.generation}`;
			case 'blocked':
				return `Not published: ${item.reason ?? 'refused'}`;
			case 'cancelled':
				return 'Cancelled';
			default:
				return 'Scheduled';
		}
	}
</script>

<section class="rounded-xl border border-border bg-card p-4 shadow-xs" aria-labelledby="schedule-heading" data-testid="schedule-panel">
	<h2 id="schedule-heading" class="flex items-center gap-2 text-base font-semibold"><CalendarClockIcon class="size-4" aria-hidden="true" />Schedule</h2>

	{#if pending}
		<div class="mt-3 space-y-2 text-sm" data-testid="schedule-pending">
			<p>
				Publishes <strong class="font-medium">{reads(pending.run_at, pending.time_zone)}</strong>
				<span class="text-muted-foreground">({pending.time_zone})</span>
			</p>
			{#if pending.time_zone !== here}
				<p class="text-xs text-muted-foreground">That's {reads(pending.run_at, here)} where you are.</p>
			{/if}
			<p class="text-xs text-muted-foreground">
				Scheduled by {pending.scheduled_by_name ?? 'someone'}, {ago(pending.created_at)}. It publishes as them, if their role still allows it and nothing
				in it has changed.
			</p>
			<Button variant="outline" size="sm" disabled={busy} onclick={() => cancel(pending)}>Cancel the schedule</Button>
		</div>
	{:else if canSchedule && changeset.status.state === 'approved'}
		<form class="mt-3 grid gap-3" onsubmit={schedule} aria-label="Schedule publishing">
			<p class="text-sm text-muted-foreground">Publish this exact version later, at a time you choose.</p>
			<div class="grid grid-cols-2 gap-2">
				<div class="grid gap-1.5">
					<Label for="schedule-date">Date</Label>
					<Input id="schedule-date" type="date" bind:value={date} required />
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
					class="h-9 w-full rounded-md border border-input bg-card px-3 text-sm shadow-xs outline-none focus-visible:border-ring focus-visible:ring-[3px] focus-visible:ring-ring/40"
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
							<input type="radio" name="schedule-choice" value={option} bind:group={choice} class="accent-primary" />
							{reads(ask.candidates[index] ?? '', zone)}
						</label>
					{/each}
				</fieldset>
			{/if}
			{#if error}<p role="alert" class="text-sm text-destructive">{error}</p>{/if}
			<Button type="submit" disabled={busy}>{ask ? 'Schedule at this moment' : 'Schedule'}</Button>
		</form>
	{:else}
		<p class="mt-2 text-sm text-muted-foreground">
			{#if changeset.status.state === 'approved'}
				Your role can't publish this, so it can't schedule it either.
			{:else if changeset.status.state === 'published'}
				It's published.
			{:else}
				Once it's approved, it can be scheduled to publish at a time you choose.
			{/if}
		</p>
	{/if}

	{#if past.length > 0}
		<ul class="mt-4 space-y-2 border-t border-dotted border-border pt-3 text-sm" data-testid="schedule-history">
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
