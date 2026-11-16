import { describe, expect, it } from 'vitest';
import { HOME_PAGE } from '@colophon/site-template';
import { renderSection, renderSections, UnrenderableSite } from '../src/index.ts';

const doc = (text: string) => ({ schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] } });

describe('the section renderer', () => {
	it('renders a hero with its optional parts', () => {
		const html = renderSection({ _block: 'hero', heading: 'Hello', tagline: 'World', label: 'Go', url: 'https://example.com/' });
		expect(html).toContain('<section class="site-section site-hero">');
		expect(html).toContain('<h1>Hello</h1>');
		expect(html).toContain('<p class="site-tagline">World</p>');
		expect(html).toContain('<a class="site-btn" href="https://example.com/">Go</a>');
	});

	it('omits the hero button when label or link is missing', () => {
		expect(renderSection({ _block: 'hero', heading: 'Hi', label: 'Go' })).not.toContain('site-btn');
		expect(renderSection({ _block: 'hero', heading: 'Hi', url: 'https://x.co' })).not.toContain('site-btn');
	});

	it('escapes everything it prints', () => {
		const html = renderSection({ _block: 'hero', heading: '<script>alert(1)</script>' });
		expect(html).not.toContain('<script>');
		expect(html).toContain('&lt;script&gt;');
	});

	it('refuses a kind it does not know', () => {
		expect(() => renderSection({ _block: 'video' })).toThrow(UnrenderableSite);
		expect(() => renderSection({})).toThrow(UnrenderableSite);
	});

	it('refuses a link address outside the policy', () => {
		expect(() => renderSection({ _block: 'hero', heading: 'Hi', label: 'Go', url: 'javascript:alert(1)' })).toThrow(UnrenderableSite);
	});

	it('renders rich text through the shared renderer', () => {
		const html = renderSection({ _block: 'rich_text', content: doc('Body <b>text</b>') });
		expect(html).toContain('site-richtext');
		expect(html).toContain('Body &lt;b&gt;text&lt;/b&gt;');
	});

	it('renders features and faq lists', () => {
		const features = renderSection({ _block: 'features', heading: 'Three', items: [{ title: 'A', text: 'one' }, { title: 'B' }] });
		expect(features).toContain('<h2>Three</h2>');
		expect((features.match(/site-card/g) ?? []).length).toBe(2);
		const faq = renderSection({ _block: 'faq', items: [{ question: 'Why?', answer: 'Because **it is**.' }] });
		expect(faq).toContain('<dt>Why?</dt>');
		expect(faq).toContain('Because <strong>it is</strong>.');
	});

	it('renders a testimonial with a byline', () => {
		const html = renderSection({ _block: 'testimonial', quote: 'Fast.', author: 'Ada', role: 'Editor' });
		expect(html).toContain('<p>Fast.</p>');
		expect(html).toContain('<footer>Ada, Editor</footer>');
	});

	it('carries style overrides as data attributes', () => {
		const html = renderSection({ _block: 'cta', heading: 'Do it', label: 'Now', url: 'https://x.co', style: { scheme: 'primary', spacing: 'spacious', align: 'center' } });
		expect(html).toContain('data-scheme="primary"');
		expect(html).toContain('data-spacing="spacious"');
		expect(html).toContain('data-align="center"');
	});

	it('renders the seeded Home page end to end', () => {
		const html = renderSections(HOME_PAGE['sections']);
		expect(html).toContain('Your site, your way');
		expect(html).toContain('Ready when you are');
	});

	it('renders nothing for an empty sections field', () => {
		expect(renderSections(undefined)).toBe('');
		expect(renderSections([])).toBe('');
	});
});
