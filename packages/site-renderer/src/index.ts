// The site renderer (W.1, D-089): a website-starter entry's parts — theme to CSS custom
// properties, sections to HTML, the menu and the document shell — as string output the Studio's
// visual canvas, the testbed and the static site build share, so the three can't drift. Strict
// like the content renderer: valid content renders deterministically, anything else is refused.

export { RENDERER_VERSION, renderSection, renderSections, type SectionContext } from './sections.ts';
export { SITE_CSS, siteCss } from './css.ts';
export { declarations, modeVars, themeCss, tokenVars, type ThemeContent, type ThemeMode, type ThemeToken } from './theme.ts';
export { pageHtml, renderNav, type NavItem, type PageParts } from './document.ts';
export { renderMarkdown } from './markdown.ts';
export { UnrenderableSite } from './errors.ts';
