// Where something stands, as one badge (D-182): each state has a tone (what its colour means), an
// icon (so colour is never the only signal) and its word from the glossary. Entries, publish
// requests, releases and schedules all speak through this map and the StatusBadge that draws it.

import type { Component } from 'svelte';
import BanIcon from '@lucide/svelte/icons/ban';
import CalendarClockIcon from '@lucide/svelte/icons/calendar-clock';
import CheckCheckIcon from '@lucide/svelte/icons/check-check';
import CircleAlertIcon from '@lucide/svelte/icons/circle-alert';
import CircleCheckIcon from '@lucide/svelte/icons/circle-check';
import CircleDashedIcon from '@lucide/svelte/icons/circle-dashed';
import CirclePlusIcon from '@lucide/svelte/icons/circle-plus';
import ClockAlertIcon from '@lucide/svelte/icons/clock-alert';
import EyeOffIcon from '@lucide/svelte/icons/eye-off';
import HourglassIcon from '@lucide/svelte/icons/hourglass';
import MessageSquareWarningIcon from '@lucide/svelte/icons/message-square-warning';
import PackageCheckIcon from '@lucide/svelte/icons/package-check';
import PencilLineIcon from '@lucide/svelte/icons/pencil-line';
import TriangleAlertIcon from '@lucide/svelte/icons/triangle-alert';
import type { ChangesetState, EntryState, Release } from './api.ts';
import { REQUEST_STATE, STATUS_WORDS } from './words.ts';

/** What a status colour means: live (readers have it), changed (live, with changes not yet
 *  published), pending (on its way: scheduled, in review, being delivered), attention (needs a
 *  person, or was taken off the site) and draft (not published). */
export type Tone = 'live' | 'changed' | 'pending' | 'attention' | 'draft';

export interface StatusLook {
	tone: Tone;
	icon: Component;
	text: string;
	/** What the badge means, said in full on hover and to assistive technology. */
	title: string;
}

const look = (tone: Tone, icon: Component, text: string, title: string): StatusLook => ({ tone, icon, text, title });

/** Every state a badge can show. */
export const STATUS = {
	// An entry, for readers (D-059's three states).
	live: look('live', CircleCheckIcon, STATUS_WORDS.live, 'Readers see this version'),
	changed: look('changed', PencilLineIcon, STATUS_WORDS.changed, 'Readers see the published version, not the changes made since'),
	draft: look('draft', CircleDashedIcon, STATUS_WORDS.draft, 'Not published'),
	// On its way.
	scheduled: look('pending', CalendarClockIcon, STATUS_WORDS.scheduled, 'Publishes by itself at the time set'),
	review: look('pending', HourglassIcon, STATUS_WORDS.review, 'Waiting for someone to approve it'),
	// What a release did to an entry.
	new: look('live', CirclePlusIcon, STATUS_WORDS.new, 'Goes live for the first time'),
	updated: look('changed', PencilLineIcon, STATUS_WORDS.updated, 'A newer version goes live'),
	taken_down: look('attention', EyeOffIcon, STATUS_WORDS.taken_down, 'Readers no longer see it'),
	// A schedule's outcome.
	published: look('live', PackageCheckIcon, STATUS_WORDS.published, 'Went live as a release'),
	published_late: look('attention', ClockAlertIcon, STATUS_WORDS.published_late, 'Went live later than scheduled'),
	not_published: look('attention', TriangleAlertIcon, STATUS_WORDS.not_published, "Didn't go live at its time"),
	cancelled: look('draft', BanIcon, STATUS_WORDS.cancelled, 'Called off before it ran'),
	// A publish request (D-044's states, in REQUEST_STATE's words).
	request_open: look('draft', CircleDashedIcon, REQUEST_STATE.open, 'Not sent for review yet'),
	request_stale: look('attention', CircleAlertIcon, REQUEST_STATE.stale, 'An entry changed after it was added'),
	request_review: look('pending', HourglassIcon, REQUEST_STATE.awaiting_review, 'Waiting for someone to approve it'),
	request_changes: look('attention', MessageSquareWarningIcon, REQUEST_STATE.changes_requested, 'A reviewer asked for changes'),
	request_approved: look('live', CircleCheckIcon, REQUEST_STATE.approved, 'Approved: ready to publish'),
	request_expired: look('attention', CircleAlertIcon, REQUEST_STATE.approval_expired, 'The approval no longer counts'),
	request_published: look('live', PackageCheckIcon, REQUEST_STATE.published, 'Went live as a release'),
	// A release's one label (D-051): solid check when published, a dashed circle while it is on
	// its way, a double check once a site check saw it.
	release_published: look('live', CircleCheckIcon, 'Published', 'Live for readers; nothing checks your website yet'),
	release_pending: look('pending', CircleDashedIcon, 'Website update pending', 'On its way to your website'),
	release_verified: look('live', CheckCheckIcon, 'Verified', 'A site check saw it on your website'),
	release_attention: look('attention', TriangleAlertIcon, 'Needs attention', "Something didn't reach your website")
} as const satisfies Record<string, StatusLook>;

export type Status = keyof typeof STATUS;

/** An entry's state (D-059) as its badge. */
export const entryStatus = (state: EntryState): Status => state;

/** A publish request's state (D-044) as its badge. */
export function requestStatus(state: ChangesetState): Status {
	switch (state) {
		case 'open':
			return 'request_open';
		case 'stale':
			return 'request_stale';
		case 'awaiting_review':
			return 'request_review';
		case 'changes_requested':
			return 'request_changes';
		case 'approved':
			return 'request_approved';
		case 'approval_expired':
			return 'request_expired';
		case 'published':
			return 'request_published';
	}
}

/** A release's label (D-051) as its badge. */
export function releaseStatus(label: Release['label']): Status {
	switch (label) {
		case 'published':
			return 'release_published';
		case 'pending':
			return 'release_pending';
		case 'verified':
			return 'release_verified';
		case 'needs_attention':
			return 'release_attention';
	}
}

/** Each tone's classes: a soft ground with a hairline ring, the word in plain ink, the icon in the
 *  tone's ink (Tailwind needs whole class names, so they are spelled out here). */
export const TONE_CLASS: Record<Tone, { badge: string; icon: string }> = {
	live: { badge: 'bg-live-soft ring-live-ring', icon: 'text-live-ink' },
	changed: { badge: 'bg-changed-soft ring-changed-ring', icon: 'text-changed-ink' },
	pending: { badge: 'bg-pending-soft ring-pending-ring', icon: 'text-pending-ink' },
	attention: { badge: 'bg-attention-soft ring-attention-ring', icon: 'text-attention-ink' },
	draft: { badge: 'bg-draft-soft ring-draft-ring', icon: 'text-draft-ink' }
};
