// `mediaFor`: a delivery row's `media` sidecar turned into the renderer's `media` callback — the
// released version by default, the current one on request, and nothing for an id the sidecar
// doesn't hold (so the renderer falls back to its bare address).

import { describe, expect, expectTypeOf, it } from 'vitest';
import { DEFAULT_SIZES, imageSource, mediaFor, type ImageSource, type MediaSidecar, type PublishedEntryView, type PublishedMedia } from '../src/index.ts';

const BASE = 'https://cms.example.com/media/v1/p1/production';
const SHA = 'b'.repeat(64);
const WIDE = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d';
const SMALL = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4e';

const media: MediaSidecar = {
	[WIDE]: {
		url: `${BASE}/${WIDE}/${SHA}`,
		current_url: `${BASE}/${WIDE}`,
		type: 'image/webp',
		width: 2400,
		height: 1600,
		// Out of order on purpose: candidates come out narrowest first.
		renditions: { w1280: `${BASE}/${WIDE}/${SHA}/w1280`, w320: `${BASE}/${WIDE}/${SHA}/w320` }
	},
	[SMALL]: {
		url: `${BASE}/${SMALL}/${SHA}`,
		current_url: `${BASE}/${SMALL}`,
		type: 'image/png',
		width: 300,
		height: 200,
		renditions: {}
	}
};

describe('mediaFor', () => {
	it('describes a released image: its versioned original, size, and renditions as width candidates', () => {
		expect(mediaFor({ media })(WIDE)).toEqual({
			src: `${BASE}/${WIDE}/${SHA}`,
			width: 2400,
			height: 1600,
			srcset: `${BASE}/${WIDE}/${SHA}/w320 320w, ${BASE}/${WIDE}/${SHA}/w1280 1280w, ${BASE}/${WIDE}/${SHA} 2400w`,
			sizes: DEFAULT_SIZES
		});
	});

	it('an image too small for any rendition gets its size and no srcset', () => {
		expect(mediaFor({ media })(SMALL)).toEqual({ src: `${BASE}/${SMALL}/${SHA}`, width: 300, height: 200 });
	});

	it('{ current: true } follows replacements: the bare address and its renditions', () => {
		expect(mediaFor({ media }, { current: true })(WIDE)).toEqual({
			src: `${BASE}/${WIDE}`,
			width: 2400,
			height: 1600,
			srcset: `${BASE}/${WIDE}/w320 320w, ${BASE}/${WIDE}/w1280 1280w, ${BASE}/${WIDE} 2400w`,
			sizes: DEFAULT_SIZES
		});
	});

	it('takes the caller’s sizes', () => {
		expect(mediaFor({ media }, { sizes: '50vw' })(WIDE)?.sizes).toBe('50vw');
	});

	it('answers undefined for what the sidecar doesn’t hold, so the renderer falls back', () => {
		expect(mediaFor({ media })('0199f0a2-6c1e-7b3a-9d42-000000000000')).toBeUndefined();
		expect(mediaFor({})(WIDE)).toBeUndefined();
		expect(mediaFor({ media: null })(WIDE)).toBeUndefined();
		expect(mediaFor(undefined)(WIDE)).toBeUndefined();
		// Never an inherited property.
		expect(mediaFor({ media })('toString')).toBeUndefined();
		expect(mediaFor({ media })('__proto__')).toBeUndefined();
	});

	it('ignores rendition names that are not widths, and any not narrower than the original', () => {
		const odd: PublishedMedia = { ...media[SMALL]!, width: 1000, renditions: { thumb: 'x', w1280: `${BASE}/a/w1280`, w320: `${BASE}/a/w320` } };
		expect(imageSource(odd).srcset).toBe(`${BASE}/a/w320 320w, ${BASE}/${SMALL}/${SHA} 1000w`);
	});

	it('takes a delivery row as the generated types describe it', () => {
		const row: PublishedEntryView = { id: WIDE, media };
		expect(mediaFor(row)(WIDE)?.width).toBe(2400);
	});

	it('is the renderer’s media option as it stands', () => {
		expectTypeOf(mediaFor({ media })).toEqualTypeOf<(assetId: string) => ImageSource | undefined>();
	});
});
