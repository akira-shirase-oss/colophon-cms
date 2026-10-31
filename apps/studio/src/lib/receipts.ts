// Words for a release's label (D-051, spec §05), shared by the Releases page and a release's
// receipt. "Verified" always says where: a probe proves what it saw, from where it looked.
// Spec §05's "Published in CMS" is said "Published in Colophon": an editor knows the product's
// name, not the acronym.

import type { Delivery, Release } from './api.ts';

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
			return `failed${delivery.last_error ? `: ${delivery.last_error}` : ''}`;
	}
}
