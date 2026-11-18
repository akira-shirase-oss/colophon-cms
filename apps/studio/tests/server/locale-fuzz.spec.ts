// The Translate dialog checks a language tag before it sends it (D-184): the check must be the
// server's, tag for tag. Each tag below goes through the client's `isLanguageTag` and `sameTag`
// and then, for real, to `POST …/localizations?locale=`: a tag the client allows is accepted, a
// tag it refuses is refused `invalid_locale`, and a language already taken — in any case — is
// refused too. A unique field that a translation would copy is the dialog's other refusal (F4).

import { expect, test } from '@playwright/test';
import { isLanguageTag, sameTag } from '../../src/lib/languages.ts';
import { api, env, key, newProject, signIn } from './helpers.ts';

const TAGS = [
	'',
	'fr',
	'FR',
	'fr-ca',
	'fr-CA',
	'zz-INVALID',
	'x'.repeat(200),
	'en-' + 'a'.repeat(8) + '-' + 'b'.repeat(8) + '-' + 'c'.repeat(8) + '-dd',
	'français',
	'ру',
	'../',
	'..%2F',
	'<script>',
	'fr-',
	'-fr',
	'f',
	'fran',
	'en-US-x-private',
	'zh-Hant',
	'de​',
	'pt_BR'
];

test('the client’s language-tag check is the server’s, tag for tag, and a taken language is refused in any case', async ({ page }) => {
	test.setTimeout(120_000);
	await signIn(page);
	const project = await newProject(page);
	const base = `${env(project)}/collections/articles/entries`;
	const made = await api(page, 'POST', `${base}?locale=en`, { title: 'Source', body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const source = made.body?.['data'].id as string;
	const taken = ['en'];
	const outcomes: string[] = [];
	for (const tag of TAGS) {
		const valid = isLanguageTag(tag);
		const already = taken.some((one) => sameTag(one, tag));
		const reply = await api(page, 'POST', `${base}/${source}/localizations?locale=${encodeURIComponent(tag)}`, undefined, { 'Idempotency-Key': key() });
		const reason = reply.body?.['error']?.details?.reason ?? null;
		outcomes.push(`${JSON.stringify(tag)}: client ${valid ? (already ? 'taken' : 'ok') : 'invalid'}, server ${reply.status} ${reason ?? ''}`);
		if (!valid) {
			expect(reply.status, outcomes.at(-1)).toBe(422);
		} else if (already) {
			expect(reply.status, outcomes.at(-1)).toBeGreaterThanOrEqual(400);
			expect(reply.status, outcomes.at(-1)).toBeLessThan(500);
		} else {
			expect(reply.status, outcomes.at(-1)).toBe(201);
			taken.push(tag);
		}
	}
	console.log(outcomes.join('\n'));
});

test('a translation that would copy a unique value is refused by the server, as the dialog says beforehand (F4)', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const definition = {
		name: 'Products',
		title_field: 'title',
		fields: [
			{ id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false },
			{ id: 'sku', label: 'Stock code', type: 'text', required: true, max_chars: 40, multiline: false, unique: true }
		]
	};
	expect((await api(page, 'POST', `${env(project)}/content-types`, { slug: 'products', definition }, { 'Idempotency-Key': key() })).status).toBe(201);
	const made = await api(page, 'POST', `${env(project)}/collections/products/entries?locale=en`, { title: 'Oilskin', sku: 'OIL-1' }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const entry = made.body?.['data'].id as string;
	const reply = await api(page, 'POST', `${env(project)}/collections/products/entries/${entry}/localizations?locale=fr`, undefined, { 'Idempotency-Key': key() });
	expect(reply.status, JSON.stringify(reply.body)).toBe(409);
	await page.goto(`/studio/projects/${project}/products/${entry}`);
	const translate = page.getByTestId('translate');
	await expect(translate).toHaveAttribute('aria-disabled', 'true');
	await expect(translate).toHaveAccessibleDescription(/A translation would copy Stock code, which no two entries may share\./);
	await translate.click();
	await expect(page.getByTestId('language-dialog')).toHaveCount(0);
});
