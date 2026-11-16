import { describe, expect, it } from 'vitest';
import { HOME_PAGE, THEME_SEED } from '@colophon/site-template';
import { pageHtml, renderNav, renderSections, siteCss, UnrenderableSite, type ThemeContent } from '../src/index.ts';

describe('the page document', () => {
	it('renders a navigation list of resolved links', () => {
		const nav = renderNav([{ label: 'Home', href: '/' }, { label: 'About', href: '/about/' }]);
		expect(nav).toContain('<nav class="site-nav">');
		expect(nav).toContain('<a href="/about/">About</a>');
		expect(renderNav([])).toBe('');
	});

	it('escapes nav labels and hrefs', () => {
		const nav = renderNav([{ label: '<b>x</b>', href: '/a?x="y"' }]);
		expect(nav).toContain('&lt;b&gt;x&lt;/b&gt;');
		expect(nav).toContain('&quot;');
	});

	it('composes a whole page: title, description, head, nav and sections', () => {
		const html = pageHtml({
			title: 'Home',
			description: 'The front page',
			head: `<style>${siteCss(THEME_SEED as ThemeContent)}</style>\n<meta name="colophon-generation" content="7">\n`,
			nav: renderNav([{ label: 'Home', href: '/' }]),
			body: renderSections(HOME_PAGE['sections'])
		});
		expect(html).toContain('<title>Home</title>');
		expect(html).toContain('<meta name="description" content="The front page">');
		expect(html).toContain('--site-primary:#6d28d9');
		expect(html).toContain('<meta name="colophon-generation" content="7">');
		expect(html).toContain('site-nav');
		expect(html).toContain('Your site, your way');
		expect(html).toContain('</html>');
	});

	it('refuses an empty title', () => {
		expect(() => pageHtml({ title: '', body: 'x' })).toThrow(UnrenderableSite);
	});
});
