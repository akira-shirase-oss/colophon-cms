import { describe, expect, it } from 'vitest';
import { renderMarkdown } from '../src/index.ts';

describe('the markdown render subset', () => {
	it('renders paragraphs on blank lines', () => {
		expect(renderMarkdown('one\n\nthree\n\n\nfour')).toBe('<p>one</p>\n<p>three</p>\n<p>four</p>');
	});

	it('renders bold, italic and code', () => {
		expect(renderMarkdown('a **b** and *c* and `d`')).toBe('<p>a <strong>b</strong> and <em>c</em> and <code>d</code></p>');
	});

	it('renders links under the shared href policy', () => {
		expect(renderMarkdown('[docs](https://example.com/x)')).toBe('<p><a href="https://example.com/x">docs</a></p>');
		expect(renderMarkdown('[local](/about)')).toBe('<p><a href="/about">local</a></p>');
	});

	it('keeps a disallowed link as literal text', () => {
		expect(renderMarkdown('[click](javascript:alert(1))')).toBe('<p>[click](javascript:alert(1))</p>');
	});

	it('never lets markup through', () => {
		const html = renderMarkdown('<script>alert(1)</script> and <img src=x>');
		expect(html).not.toContain('<script>');
		expect(html).not.toContain('<img');
		expect(html).toContain('&lt;script&gt;');
	});

	it('renders nothing for empty input', () => {
		expect(renderMarkdown('')).toBe('');
		expect(renderMarkdown('  \n\n  ')).toBe('');
	});
});
