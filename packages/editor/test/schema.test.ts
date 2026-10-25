import { describe, expect, test } from 'vitest';
import { isAllowedHref, isValidAlt, LIMITS } from '../src/index.ts';

// The same table runs in Rust (colophon-domain, rich_text::href tests): the two must agree.
const allowed = [
	'https://example.com',
	'https://example.com/a?b=c#d',
	'HTTPS://EXAMPLE.COM',
	'http://example.com',
	'https:example.com',
	'mailto:editor@example.com',
	'mailto:',
	'/about',
	'/',
	'/a/b?c=d#e',
	'#links',
	'#',
	'https://例え.jp/パス',
	`https://example.com/${'a'.repeat(LIMITS.maxHrefLength - 20)}`
];
const refused = [
	'',
	'javascript:alert(1)',
	'JavaScript:alert(1)',
	'data:text/html,x',
	'vbscript:x',
	'file:///etc/passwd',
	'ftp://example.com',
	'//evil.example',
	'/\t/evil.example',
	'/\\evil.example',
	'\\\\evil.example',
	' https://example.com',
	'https://example.com/a b',
	'java\nscript:alert(1)',
	'https://',
	'relative/path',
	'example.com',
	`https://example.com/${'a'.repeat(LIMITS.maxHrefLength)}`
];

describe('links', () => {
	test.each(allowed)('allows %j', (href) => expect(isAllowedHref(href)).toBe(true));
	test.each(refused)('refuses %j', (href) => expect(isAllowedHref(href)).toBe(false));
});

describe('alt text', () => {
	test('must not be blank', () => {
		expect(isValidAlt('A diagram')).toBe(true);
		for (const blank of ['', ' ', '\t\n', ' ', '﻿']) expect(isValidAlt(blank)).toBe(false);
	});

	test('is limited in code points, not UTF-16 units', () => {
		// 300 emoji are 600 UTF-16 units but 300 code points: allowed.
		expect(isValidAlt('🚀'.repeat(LIMITS.maxAltLength))).toBe(true);
		expect(isValidAlt('🚀'.repeat(LIMITS.maxAltLength + 1))).toBe(false);
	});
});
