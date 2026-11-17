// Images inside rich text, end to end through the real HTTP API: an image node names a real
// upload — tracked like an asset field's — and the release records the version it published, so
// delivery hands a site the immutable address, the size and the renditions (the `media` sidecar).
// Each step is written down as it happens, so the page and its Playwright spec read the same
// words.

import { call, csrfToken, detailsOf, envelopeOf, type Reply } from './api.ts';

/** A 400×4 PNG: wider than `w320`, so the worker writes one rendition. */
export const WIDE_PNG = new Uint8Array([
	137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 1, 144, 0, 0, 0, 4, 8, 2, 0, 0, 0, 47, 93, 254, 122, 0, 0, 0, 42, 73,
	68, 65, 84, 120, 156, 237, 206, 65, 9, 0, 48, 12, 4, 176, 74, 159, 243, 206, 194, 253, 142, 66, 32, 2, 50, 243, 22, 224, 134, 254, 0,
	32, 212, 31, 0, 132, 250, 3, 128, 204, 7, 24, 121, 90, 72, 220, 227, 41, 34, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130
]);

/** One image as a delivery row's sidecar records it. */
export interface PublishedMedia {
	url: string;
	current_url: string;
	type: string;
	width: number;
	height: number;
	renditions: Record<string, string>;
}

/** What the journey ends with: the delivered image, for the page to draw. */
export interface Delivered {
	media: PublishedMedia;
	alt: string;
}

export function describe(reply: Reply): string {
	const error = envelopeOf(reply.body);
	const details = detailsOf(reply.body);
	return `${reply.status} ${error?.code ?? ''}${details ? ` (${details.reason}${details.pointer ? ` at ${details.pointer}` : ''})` : ''}`
		.replace(/\s+/g, ' ')
		.trim();
}

function image(assetId: string, alt: string) {
	return { type: 'image', attrs: { assetId, alt } };
}

function body(...content: unknown[]) {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content } };
}

/** Runs the journey on `project`, noting every step; the delivered image when it got that far. */
export async function imageJourney(project: string, note: (step: string) => void): Promise<Delivered | null> {
	const env = `/api/v1/projects/${project}/environments/production`;
	const json = { 'Content-Type': 'application/json', 'X-CSRF-Token': csrfToken() };
	const once = () => ({ ...json, 'Idempotency-Key': crypto.randomUUID() });
	const stop = (step: string): null => {
		note(step);
		return null;
	};

	// 1. A real upload, settled by the worker.
	const uploaded = await call('POST', `${env}/assets`, {
		headers: { 'Content-Type': 'image/png', 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID(), 'X-Colophon-File-Name': 'press.png' },
		body: WIDE_PNG as unknown as BodyInit
	});
	if (uploaded.status !== 202) return stop(`upload: ${describe(uploaded)}`);
	const asset = (uploaded.body as { data: { id: string } }).data.id;
	let sha = '';
	const deadline = Date.now() + 15_000;
	for (;;) {
		const read = await call('GET', `${env}/assets/${asset}`);
		const record = (read.body as { data?: { state: string; sha256: string | null; width: number | null; height: number | null } } | null)?.data;
		if (record !== undefined && record.state !== 'quarantined') {
			note(`upload: ${record.state} ${record.width ?? '?'}×${record.height ?? '?'}`);
			if (record.state !== 'usable') return null;
			sha = record.sha256 ?? '';
			break;
		}
		if (Date.now() > deadline) return stop('upload: still quarantined after 15s');
		await new Promise((wait) => setTimeout(wait, 200));
	}

	// 2. An image naming no upload of this environment is refused at its place, not stored.
	const unknown = await call('POST', `${env}/collections/articles/entries`, {
		headers: once(),
		body: JSON.stringify({ title: 'Made-up image', body: body(image(crypto.randomUUID(), 'Nothing')) })
	});
	note(`unknown image: ${describe(unknown)}`);

	// 3. The real one saves; the same image twice is one reference.
	const saved = await call('POST', `${env}/collections/articles/entries`, {
		headers: once(),
		body: JSON.stringify({
			title: 'Pictured',
			body: body(image(asset, 'The press, from the left'), { type: 'paragraph', content: [{ type: 'text', text: 'Between.' }] }, image(asset, 'The press again'))
		})
	});
	note(`saved: ${describe(saved)}`);
	if (saved.status !== 201) return null;
	const entry = (saved.body as { data: { id: string } }).data.id;

	// 4. The upload knows where it is used, and can't be deleted from under the draft.
	const references = await call('GET', `${env}/assets/${asset}/references`);
	const named = (references.body as { data?: { field: string; in_draft: boolean }[] } | null)?.data ?? [];
	note(`used in: ${named.map((reference) => `${reference.field}${reference.in_draft ? ' (draft)' : ''}`).join(', ') || describe(references)}`);
	const removed = await call('DELETE', `${env}/assets/${asset}`, { headers: { 'X-CSRF-Token': csrfToken(), 'Idempotency-Key': crypto.randomUUID() } });
	note(`delete while used: ${describe(removed)}`);

	// 5. Published through a changeset, as anyone publishes.
	const made = await call('POST', `${env}/changesets`, { headers: once(), body: JSON.stringify({ title: 'Pictured', entries: [entry] }) });
	const set = (made.body as { data?: { id: string; hash: string } } | null)?.data;
	if (set === undefined) return stop(`changeset: ${describe(made)}`);
	const guarded = { ...json, 'If-Match': `"${set.hash}"` };
	await call('POST', `${env}/changesets/${set.id}/submit`, { headers: guarded });
	await call('POST', `${env}/changesets/${set.id}/reviews`, { headers: guarded, body: JSON.stringify({ decision: 'approve' }) });
	const published = await call('POST', `${env}/changesets/${set.id}/publish`, {
		headers: { ...guarded, 'Idempotency-Key': crypto.randomUUID() },
		body: JSON.stringify({ base_generation: 0 })
	});
	note(`published: ${describe(published)}`);
	if (published.status !== 201) return null;

	// 6. Delivery hands a site the version the release published.
	const delivered = await call('GET', `/content/v1/${project}/production/articles/${entry}`);
	const media = (delivered.body as { data?: { media?: Record<string, PublishedMedia> } } | null)?.data?.media?.[asset];
	if (media === undefined) return stop(`delivered: ${describe(delivered)}, no media for the image`);
	note(
		`delivered: ${media.url.endsWith(`/${asset}/${sha}`) ? 'versioned url' : `unexpected url ${media.url}`}, ` +
			`current url ${media.current_url.endsWith(`/${asset}`) ? 'bare' : 'unexpected'}, ` +
			`${media.type} ${media.width}×${media.height}, renditions ${Object.keys(media.renditions).sort().join(' ') || 'none'}`
	);

	// 7. The versioned address serves those bytes, cacheable for a year.
	const served = await fetch(`/server${new URL(media.url).pathname}`, { cache: 'no-store' });
	note(`served: ${served.status} ${served.headers.get('content-type') ?? ''} ${served.headers.get('cache-control') ?? ''}`.trim());
	return { media, alt: 'The press, from the left' };
}
