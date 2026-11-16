// The site stylesheet (W.1): the one CSS file every page links, beside the theme's :root
// variables. It reads --site-* tokens only — a republished theme reskins the whole site without
// a markup change — and per-section overrides ride the data attributes the section renderer
// emits. No inline styles anywhere, so pages hold under a strict CSP.

import type { ThemeContent } from './theme.ts';
import { themeCss } from './theme.ts';

export const SITE_CSS = `*,*::before,*::after{box-sizing:border-box}
body{margin:0;font-family:var(--site-font-body);font-size:var(--site-base-size);line-height:1.6;color:var(--site-text);background:var(--site-surface)}
h1,h2,h3{font-family:var(--site-font-heading);line-height:1.15;margin:0 0 .75rem}
h1{font-size:2.5rem}h2{font-size:1.75rem}h3{font-size:1.125rem;margin-bottom:.375rem}
p{margin:0 0 .75rem}a{color:var(--site-primary)}
.site-nav{padding:1rem 1.5rem;border-bottom:1px solid color-mix(in srgb,var(--site-muted) 25%,transparent)}
.site-nav-list{display:flex;gap:1.25rem;list-style:none;margin:0;padding:0}
.site-nav-list a{color:var(--site-text);text-decoration:none}
.site-nav-list a:hover{color:var(--site-primary)}
.site-section{padding:3rem 1.5rem}
.site-section[data-spacing=compact]{padding:1.5rem}
.site-section[data-spacing=spacious]{padding:5rem 1.5rem}
.site-section[data-align=center]{text-align:center}
.site-section[data-align=center] :is(.site-hero-inner,.site-prose,.site-faq-list,blockquote,.site-cta-inner){margin-inline:auto}
.site-section[data-align=end]{text-align:end}
.site-section[data-scheme=primary]{background:var(--site-primary);color:#fff}
.site-section[data-scheme=secondary]{background:var(--site-secondary);color:#fff}
.site-section[data-scheme=invert]{background:var(--site-text);color:var(--site-surface)}
.site-section[data-scheme=surface]{background:var(--site-surface)}
.site-section[data-scheme] .site-tagline{color:inherit;opacity:.85}
.site-hero-inner,.site-cta-inner{max-width:44rem;margin:0 auto}
.site-tagline{font-size:1.125rem;color:var(--site-muted);margin-bottom:1.5rem}
.site-btn{display:inline-block;background:var(--site-primary);color:#fff;padding:.625rem 1.25rem;border-radius:var(--site-radius-md);box-shadow:var(--site-shadow);text-decoration:none;font-weight:600}
.site-btn:hover{filter:brightness(1.08)}
.site-section[data-scheme=primary] .site-btn,.site-section[data-scheme=secondary] .site-btn{background:var(--site-surface);color:var(--site-text)}
.site-prose{max-width:44rem;margin:0 auto}
.site-feature-grid{display:grid;grid-template-columns:repeat(auto-fit,minmax(14rem,1fr));gap:1.25rem;list-style:none;margin:1.5rem 0 0;padding:0}
.site-card{background:color-mix(in srgb,var(--site-surface) 90%,var(--site-text));border-radius:var(--site-radius-lg);padding:1.25rem;box-shadow:var(--site-shadow)}
.site-section[data-scheme] .site-card{background:var(--site-surface);color:var(--site-text)}
.site-testimonial blockquote{max-width:42rem;margin:0 auto;font-size:1.25rem}
.site-testimonial footer{color:var(--site-muted);margin-top:1rem;font-size:1rem}
.site-section[data-scheme] .site-testimonial footer{color:inherit;opacity:.8}
.site-faq-list{max-width:44rem;margin:1.5rem auto 0}
.site-faq-item{padding:1.25rem 0;border-bottom:1px solid color-mix(in srgb,var(--site-muted) 30%,transparent)}
.site-faq-item dt{font-weight:600}
.site-faq-item dd{margin:.5rem 0 0}
.site-faq-item dd p{margin-bottom:.5rem}
`;

/** The whole stylesheet for a theme entry: its :root variables, then the shared rules. */
export function siteCss(theme: ThemeContent): string {
	return `${themeCss(theme)}\n${SITE_CSS}`;
}
