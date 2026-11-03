// The section library's renderer (W.1): one `_block` kind to one HTML fragment. The output is
// classes and data attributes only — no inline styles — so it lives under the same strict
// Content Security Policy as the rich-text renderer (R2), and every look comes from the theme's
// --site-* variables and the shared stylesheet. Unknown kinds are refused: stored content has
// already passed schema validation, so a kind the renderer doesn't know is a bug, not a choice.

import { renderToHtml } from '@colophon/content-renderer';
import { isAllowedHref } from '@colophon/editor/schema';
import { escapeAttribute, escapeText } from './escape.ts';
import { UnrenderableSite } from './errors.ts';
import { renderMarkdown } from './markdown.ts';

/** Recorded with every build, so a change to the output is a visible version change. */
export const RENDERER_VERSION = 1;

export interface SectionContext {
	/** The public URL of a media-library asset, for images inside rich text. */
	assetUrl: (assetId: string) => string;
	/** Mark every section's root with its index (`data-i`), for the Studio's click-to-edit. */
	annotate?: boolean;
}

const DEFAULT_CONTEXT: SectionContext = { assetUrl: (id) => `/assets/${id}` };

type Block = Record<string, unknown>;

const text = (block: Block, key: string): string | undefined => {
	const value = block[key];
	if (value === undefined || value === null) return undefined;
	if (typeof value !== 'string') throw new UnrenderableSite(`${key} is not text`);
	return value;
};

const requiredText = (block: Block, key: string): string => {
	const value = text(block, key);
	if (value === undefined) throw new UnrenderableSite(`${key} is missing`);
	return value;
};

const items = (block: Block, key: string): Block[] => {
	const value = block[key];
	if (value === undefined || value === null) return [];
	if (!Array.isArray(value)) throw new UnrenderableSite(`${key} is not a list`);
	return value as Block[];
};

/** A section link, under the same href policy the rich text enforces. */
function link(block: Block, labelKey: string, urlKey: string): string {
	const label = text(block, labelKey);
	const url = text(block, urlKey);
	if (label === undefined || label === '' || url === undefined || url === '') return '';
	if (!isAllowedHref(url)) throw new UnrenderableSite(`a section link's address is not allowed`);
	return `<a class="site-btn" href="${escapeAttribute(url)}">${escapeText(label)}</a>`;
}

/** The shared style override: scheme, spacing and alignment as data attributes the stylesheet
 *  keys off. Anything unset follows the theme. */
function styleAttributes(block: Block): string {
	const style = block['style'];
	if (style === undefined || style === null) return '';
	if (typeof style !== 'object' || Array.isArray(style)) throw new UnrenderableSite('style is not the shared component');
	const attrs: string[] = [];
	for (const key of ['scheme', 'spacing', 'align'] as const) {
		const value = (style as Block)[key];
		if (value === undefined || value === null || value === '') continue;
		if (typeof value !== 'string') throw new UnrenderableSite(`style ${key} is not a choice`);
		attrs.push(`data-${key}="${escapeAttribute(value)}"`);
	}
	return attrs.length === 0 ? '' : ` ${attrs.join(' ')}`;
}

const open = (block: Block, name: string, index?: number): string =>
	`<section class="site-section site-${name}"${index === undefined ? '' : ` data-i="${index}"`}${styleAttributes(block)}>`;

function hero(block: Block, _ctx: SectionContext, index?: number): string {
	const heading = requiredText(block, 'heading');
	const tagline = text(block, 'tagline');
	return `${open(block, 'hero', index)}<div class="site-hero-inner">
<h1>${escapeText(heading)}</h1>
${tagline === undefined || tagline === '' ? '' : `<p class="site-tagline">${escapeText(tagline)}</p>\n`}${link(block, 'label', 'url')}
</div></section>`;
}

function richText(block: Block, ctx: SectionContext, index?: number): string {
	const content = block['content'];
	if (content === undefined) throw new UnrenderableSite('content is missing');
	const body = renderToHtml(content as Parameters<typeof renderToHtml>[0], { assetUrl: ctx.assetUrl });
	return `${open(block, 'richtext', index)}<div class="site-prose">\n${body}\n</div></section>`;
}

function features(block: Block, _ctx: SectionContext, index?: number): string {
	const heading = text(block, 'heading');
	const cards = items(block, 'items').map((item) => {
		const title = requiredText(item, 'title');
		const body = text(item, 'text');
		return `<li class="site-card"><h3>${escapeText(title)}</h3>${body === undefined || body === '' ? '' : `\n<p>${escapeText(body)}</p>`}</li>`;
	});
	return `${open(block, 'features', index)}${heading === undefined || heading === '' ? '' : `<h2>${escapeText(heading)}</h2>\n`}
<ul class="site-feature-grid">
${cards.join('\n')}
</ul></section>`;
}

function testimonial(block: Block, _ctx: SectionContext, index?: number): string {
	const quote = requiredText(block, 'quote');
	const author = text(block, 'author');
	const role = text(block, 'role');
	const byline = [author, role].filter((part) => part !== undefined && part !== '').map((part) => escapeText(part as string)).join(', ');
	return `${open(block, 'testimonial', index)}<blockquote>
<p>${escapeText(quote)}</p>
${byline === '' ? '' : `<footer>${byline}</footer>\n`}</blockquote></section>`;
}

function cta(block: Block, _ctx: SectionContext, index?: number): string {
	const heading = requiredText(block, 'heading');
	const body = text(block, 'text');
	return `${open(block, 'cta', index)}<div class="site-cta-inner">
<h2>${escapeText(heading)}</h2>
${body === undefined || body === '' ? '' : `<p>${escapeText(body)}</p>\n`}${link(block, 'label', 'url')}
</div></section>`;
}

function faq(block: Block, _ctx: SectionContext, index?: number): string {
	const heading = text(block, 'heading');
	const rows = items(block, 'items').map((item) => {
		const question = requiredText(item, 'question');
		const answer = requiredText(item, 'answer');
		return `<div class="site-faq-item"><dt>${escapeText(question)}</dt>\n<dd>${renderMarkdown(answer)}</dd></div>`;
	});
	return `${open(block, 'faq', index)}${heading === undefined || heading === '' ? '' : `<h2>${escapeText(heading)}</h2>\n`}
<dl class="site-faq-list">
${rows.join('\n')}
</dl></section>`;
}

const KINDS: Record<string, (block: Block, ctx: SectionContext, index?: number) => string> = {
	hero,
	rich_text: richText,
	features,
	testimonial,
	cta,
	faq
};

/** One section. `block` is one item of a page's `sections` field — `_block` names its kind. */
export function renderSection(block: Block, ctx: SectionContext = DEFAULT_CONTEXT, index?: number): string {
	const kind = block['_block'];
	if (typeof kind !== 'string' || !(kind in KINDS)) throw new UnrenderableSite(`a section kind ${JSON.stringify(kind)} is not one the site knows`);
	return (KINDS[kind] as (block: Block, ctx: SectionContext, index?: number) => string)(block, ctx, ctx.annotate === true ? index : undefined);
}

/** A page's `sections` field to HTML, top to bottom. */
export function renderSections(blocks: unknown, ctx: SectionContext = DEFAULT_CONTEXT): string {
	if (blocks === undefined || blocks === null) return '';
	if (!Array.isArray(blocks)) throw new UnrenderableSite('sections is not a list');
	return blocks.map((block, index) => renderSection(block as Block, ctx, index)).join('\n');
}
