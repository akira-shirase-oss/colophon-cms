// Words for a release's label (D-051, spec §05), shared by the Releases page and a release's
// receipt. "Verified" always says where: a probe proves what it saw, from where it looked.
// Spec §05's "Published in CMS" is said "Published in Colophon" (D-082): an editor knows the
// product's name, not the acronym.

import type { Delivery, Release } from './api.ts';
import { INTERNAL_TERMS } from './words.ts';

export function labelText(release: Pick<Release, 'label' | 'locations'>): string {
	switch (release.label) {
		case 'published':
			return 'Published in Colophon';
		case 'pending':
			return 'Website update pending';
		case 'verified':
			return `Verified at ${release.locations} ${release.locations === 1 ? 'location' : 'locations'}`;
		case 'needs_attention':
			return 'Needs attention';
	}
}

export function deliveryText(delivery: Delivery): string {
	switch (delivery.state) {
		case 'done':
			return delivery.kind === 'probe' ? 'saw this release' : delivery.kind === 'cdn' ? 'clearing accepted' : 'delivered';
		case 'covered':
			return `covered by release ${delivery.covered_by ?? '?'}`;
		case 'pending':
			return delivery.kind === 'probe' ? 'still looking' : 'on its way';
		case 'failed':
			return `failed${delivery.last_error ? `: ${failureText(delivery.last_error)}` : ''}`;
	}
}

/** Why an attempt to reach the website failed, in plain words, from what the worker recorded:
 *  `HTTP 404`, `timed out`, `could not connect`, a probe's "saw release 6, not yet 7"… */
export function failureText(error: string): string {
	const status = /^HTTP (\d{3})$/.exec(error.trim());
	if (status?.[1] !== undefined) {
		const code = Number(status[1]);
		return code >= 500 ? `your website had a problem (HTTP ${code})` : `your website refused it (HTTP ${code})`;
	}
	if (error === 'timed out') return "your website didn't answer in time";
	if (error === 'could not connect') return "couldn't reach your website";
	const behind = /^saw release (\d+), not yet (\d+)$/.exec(error);
	if (behind?.[1] !== undefined) return `the live site still showed release ${behind[1]}`;
	if (error === 'the answer names no release') return "the page it checked doesn't say which release it shows";
	return INTERNAL_TERMS.test(error) ? 'it was refused' : error;
}
