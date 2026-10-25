import { join } from 'node:path';
import { describe, expect, test } from 'vitest';
import type { RichTextValue } from '@colophon/editor/stored';
import { renderToHtml, RENDERER_VERSION, UnrenderableContent } from '../src/index.ts';
import { invalid, valid } from './fixtures.ts';
import { unsafeParts } from './safety.ts';

const ASSETS = 'https://assets.example/';
const options = { assetUrl: (id: string) => `${ASSETS}${id}` };
const rendered = join(import.meta.dirname, '../../../fixtures/rich-text/v1/rendered');

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

describe('every invalid fixture is refused, or rendered without anything unsafe', () => {
	test.each(invalid.map((fixture) => [fixture.name, fixture] as const))('%s', (_name, fixture) => {
		let html: string;
		try {
			html = renderToHtml(fixture.value, options);
		} catch (error) {
			expect(error).toBeInstanceOf(UnrenderableContent);
			return;
		}
		expect(unsafeParts(html, ASSETS)).toEqual([]);
		for (const forbidden of ['<script', 'onclick', 'style=', 'evil.example', 'javascript:', 'data:', 'iframe']) {
			expect(html, forbidden).not.toContain(forbidden);
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
	expect(RENDERER_VERSION).toBe(1);
});
