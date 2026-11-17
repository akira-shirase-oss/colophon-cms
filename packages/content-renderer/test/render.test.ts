import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import type { RichTextValue } from '@colophon/editor/stored';
import { type ImageSource, renderToHtml, RENDERER_VERSION, UnrenderableContent } from '../src/index.ts';
import { invalid, valid } from './fixtures.ts';
import { unsafeParts } from './safety.ts';

const ASSETS = 'https://assets.example/';
const options = { assetUrl: (id: string) => `${ASSETS}${id}` };
const rendered = join(import.meta.dirname, '../../../fixtures/rich-text/v1/rendered');
const renderedMedia = join(import.meta.dirname, '../../../fixtures/rich-text/v1/rendered-media');

/** What a delivery row's sidecar gives a site for every image (renderer v2): the versioned
 *  original with its size, and its renditions as width candidates. */
const SHA = 'a'.repeat(64);
const described: ImageSource = {
	src: `${ASSETS}media/v1/p/production/ID/${SHA}`,
	width: 2400,
	height: 1600,
	srcset: `${ASSETS}media/v1/p/production/ID/${SHA}/w320 320w, ${ASSETS}media/v1/p/production/ID/${SHA}/w1280 1280w, ${ASSETS}media/v1/p/production/ID/${SHA} 2400w`,
	sizes: '(max-width: 1280px) 100vw, 1280px'
};
const withMedia = {
	...options,
	media: (id: string): ImageSource => ({
		...described,
		src: described.src.replace('ID', id),
		srcset: described.srcset?.replaceAll('ID', id)
	})
};

function doc(...content: unknown[]): RichTextValue {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content } } as RichTextValue;
}

describe('every valid fixture renders to its reviewed output, and the output is safe', () => {
	test.each(valid.map((fixture) => [fixture.name, fixture] as const))('%s', async (_name, fixture) => {
		const html = renderToHtml(fixture.value, options);
		expect(unsafeParts(html, ASSETS)).toEqual([]);
		await expect(html + '\n').toMatchFileSnapshot(join(rendered, `${fixture.name}.html`));
	});
});

describe('renderer v2: every valid fixture with images renders its reviewed sized, responsive output', () => {
	const withImages = valid.filter((fixture) => JSON.stringify(fixture.value).includes('"type":"image"'));
	test('the fixtures include images', () => {
		expect(withImages.length).toBeGreaterThan(0);
	});
	test.each(withImages.map((fixture) => [fixture.name, fixture] as const))('%s', async (_name, fixture) => {
		const html = renderToHtml(fixture.value, withMedia);
		expect(unsafeParts(html, ASSETS)).toEqual([]);
		expect(html).toContain(' width="2400" height="1600"');
		await expect(html + '\n').toMatchFileSnapshot(join(renderedMedia, `${fixture.name}.html`));
	});

	test('without media, or for an asset media does not know, the bare address as before', () => {
		for (const fixture of valid) {
			expect(renderToHtml(fixture.value, { ...options, media: () => undefined })).toBe(renderToHtml(fixture.value, options));
		}
	});
});

describe('renderer v2: what media hands back is checked and escaped like everything else', () => {
	const ID = '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d';
	const one = (source: ImageSource) => renderToHtml(doc({ type: 'image', attrs: { assetId: ID, alt: 'A mark' } }), { ...options, media: () => source });

	test('a quote breaking out of srcset or sizes is escaped', () => {
		const html = one({ src: `${ASSETS}a`, srcset: `${ASSETS}a?x="onerror=alert(1) 320w`, sizes: '"><script>alert(1)</script>' });
		expect(html).toContain('srcset="https://assets.example/a?x=&quot;onerror=alert(1) 320w"');
		expect(html).toContain('sizes="&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;"');
		expect(unsafeParts(html, ASSETS)).toEqual([]);
	});

	test.each([
		['a javascript: src', { src: 'javascript:alert(1)' }],
		['a data: src', { src: 'data:image/svg+xml,<svg onload=alert(1)>' }],
		['a protocol-relative src', { src: '//evil.example/x.png' }],
		['a src with whitespace', { src: 'https://assets.example/a onerror=alert(1)' }],
		['a javascript: srcset candidate', { src: `${ASSETS}a`, srcset: `${ASSETS}a 320w, javascript:alert(1) 1280w` }],
		['a srcset candidate without a descriptor', { src: `${ASSETS}a`, srcset: `${ASSETS}a` }],
		['a srcset candidate with extra words', { src: `${ASSETS}a`, srcset: `${ASSETS}a 320w onerror=alert(1)` }]
	] as const)('%s is refused', (_what, source) => {
		expect(() => one(source as ImageSource)).toThrow(UnrenderableContent);
	});

	test('dimensions that are not positive whole pixels are left out, and both or neither are emitted', () => {
		for (const [width, height] of [[0, 10], [10.5, 10], [-1, 10], [Number.NaN, 10], [10, undefined]] as const) {
			const html = one({ src: `${ASSETS}a`, width, height } as ImageSource);
			expect(html).not.toContain('width=');
			expect(html).not.toContain('height=');
		}
		expect(one({ src: `${ASSETS}a`, width: 640, height: 480 })).toBe(
			'<figure><img src="https://assets.example/a" width="640" height="480" alt="A mark" loading="lazy" decoding="async"></figure>'
		);
	});

	test('sizes without srcset says nothing, so it is left out', () => {
		expect(one({ src: `${ASSETS}a`, sizes: '100vw' })).not.toContain('sizes=');
	});

	test('a root-relative path is an image URL too', () => {
		expect(one({ src: '/media/v1/p/production/a' })).toContain('src="/media/v1/p/production/a"');
	});
});

describe('every invalid fixture is refused, or rendered without anything unsafe', () => {
	test.each(invalid.map((fixture) => [fixture.name, fixture] as const))('%s', (_name, fixture) => {
		for (const rendering of [options, withMedia]) {
			let html: string;
			try {
				html = renderToHtml(fixture.value, rendering);
			} catch (error) {
				expect(error).toBeInstanceOf(UnrenderableContent);
				continue;
			}
			expect(unsafeParts(html, ASSETS)).toEqual([]);
			for (const forbidden of ['<script', 'onclick', 'style=', 'evil.example', 'javascript:', 'data:', 'iframe']) {
				expect(html, forbidden).not.toContain(forbidden);
			}
		}
	});
});

describe('hostile text is escaped, never interpreted', () => {
	test('markup in text', () => {
		const html = renderToHtml(doc({ type: 'paragraph', content: [{ type: 'text', text: '<script>alert(1)</script> & </p>' }] }), options);
		expect(html).toBe('<p>&lt;script&gt;alert(1)&lt;/script&gt; &amp; &lt;/p&gt;</p>');
	});

	test('a quote breaking out of alt text', () => {
		const html = renderToHtml(
			doc({ type: 'image', attrs: { assetId: '0199f0a2-6c1e-7b3a-9d42-5e8f1a2b3c4d', alt: '"><script>alert(1)</script>' } }),
			options
		);
		expect(html).toContain('alt="&quot;&gt;&lt;script&gt;alert(1)&lt;/script&gt;"');
		expect(unsafeParts(html, ASSETS)).toEqual([]);
	});

	test('escaping a code block', () => {
		const html = renderToHtml(doc({ type: 'codeBlock', content: [{ type: 'text', text: '</code></pre><img src=x onerror=alert(1)>' }] }), options);
		expect(html).toBe('<pre><code>&lt;/code&gt;&lt;/pre&gt;&lt;img src=x onerror=alert(1)&gt;</code></pre>');
		expect(unsafeParts(html, ASSETS)).toEqual([]);
	});

	test('a quote inside an allowed link', () => {
		const href = 'https://example.com/"onmouseover="alert(1)';
		const html = renderToHtml(doc({ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'link', attrs: { href } }] }] }), options);
		expect(html).toBe('<p><a href="https://example.com/&quot;onmouseover=&quot;alert(1)" rel="noopener noreferrer">x</a></p>');
		expect(unsafeParts(html, ASSETS)).toEqual([]);
	});
});

describe('the renderer refuses rather than guesses', () => {
	test.each([
		['an unknown node', doc({ type: 'iframe' })],
		['a javascript: link', doc({ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'link', attrs: { href: 'javascript:alert(1)' } }] }] })],
		['a level-1 heading', doc({ type: 'heading', attrs: { level: 1 } })],
		['an unknown mark', doc({ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'underline' }] }] })],
		['marks in a code block', doc({ type: 'codeBlock', content: [{ type: 'text', text: 'x', marks: [{ type: 'bold' }] }] })],
		['an empty document', doc()],
		['no document', { schema: 'colophon/rich-text', version: 1 } as unknown as RichTextValue],
		['content that is not a list', { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: 'x' } } as unknown as RichTextValue],
		['a node that is not an object', doc('paragraph')],
		['marks that are not a list', doc({ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: 'bold' }] })]
	])('%s', (_what, value) => {
		expect(() => renderToHtml(value, options)).toThrow(UnrenderableContent);
	});
});

test('marks nest in canonical order, the first outermost', () => {
	const html = renderToHtml(
		doc({ type: 'paragraph', content: [{ type: 'text', text: 'x', marks: [{ type: 'link', attrs: { href: '/a' } }, { type: 'bold' }, { type: 'italic' }] }] }),
		options
	);
	expect(html).toBe('<p><a href="/a"><strong><em>x</em></strong></a></p>');
});

test('the renderer has a version', () => {
	expect(RENDERER_VERSION).toBe(2);
});
