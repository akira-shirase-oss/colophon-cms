// The colophon/rich-text v1 rules the editor needs, as data. The normative description is
// docs/formats/rich-text-v1.md; the Rust validator (colophon-domain) is the authority.

export const RICH_TEXT_SCHEMA = 'colophon/rich-text';
export const RICH_TEXT_VERSION = 1;

export const HEADING_LEVELS = [2, 3, 4] as const;
export type HeadingLevel = (typeof HEADING_LEVELS)[number];

export const CALLOUT_TONES = ['note', 'tip', 'warning'] as const;
export type CalloutTone = (typeof CALLOUT_TONES)[number];

export const LIMITS = {
	maxDepth: 16,
	maxBytes: 262_144,
	maxNodes: 10_000,
	maxAltLength: 300,
	maxHrefLength: 2048
} as const;

export const CODE_LANGUAGE = /^[a-z0-9][a-z0-9+#.-]{0,19}$/;
export const ASSET_ID = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;

/**
 * https:, http: and mailto: URLs; paths starting with exactly one `/`; fragments starting with
 * `#`. Control characters, whitespace and backslashes are refused anywhere: browsers strip tabs
 * and newlines and read `\` as `/`, so `/<tab>/evil.example` would lead to another site.
 * Lengths are in characters (code points), as in the Rust validator.
 */
export function isAllowedHref(href: string): boolean {
	const length = [...href].length;
	if (length === 0 || length > LIMITS.maxHrefLength) return false;
	if (/[\u0000-\u0020\u007f\\]/.test(href)) return false;
	if (href.startsWith('#')) return true;
	if (href.startsWith('/')) return !href.startsWith('//');
	let url: URL;
	try {
		url = new URL(href);
	} catch {
		return false;
	}
	return url.protocol === 'https:' || url.protocol === 'http:' || url.protocol === 'mailto:';
}

/** Alt text rules: not only whitespace, at most 300 characters (code points). */
export function isValidAlt(alt: string): boolean {
	return alt.trim() !== '' && [...alt].length <= LIMITS.maxAltLength;
}

export function isCalloutTone(value: unknown): value is CalloutTone {
	return typeof value === 'string' && (CALLOUT_TONES as readonly string[]).includes(value);
}
