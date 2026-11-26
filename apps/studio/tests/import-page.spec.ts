// The Import content page against answers the server would give (no colophon-server): the
// source choices and what isn't supported, a Markdown folder from files to drafts (front matter
// matched to fields, the body in the Markdown field, the URL name from the file's name), a mixed
// Sanity export narrowed to one kind, a file that isn't what its source says, a long file sent
// in batches, and a run that stops part way reported as a part, never a success. The mock judges
// each item roughly as the server does (a missing title, a name the type doesn't have), and
// records every body it was sent. tests/server/transfer.spec.ts covers the real import route.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, type Route, test } from '@playwright/test';
import { internalTerms } from './server/helpers.ts';

const PROJECT = '0b9a3c52-6f0e-4d7e-9a51-3f1c2d4e5a60';
const P = `/api/v1/projects/${PROJECT}/environments/production`;
const URL = `/studio/projects/${PROJECT}/import`;
const AT = '2026-11-20T12:00:00Z';
// SCREENSHOTS=dir saves the page's states at the width the test runs at, for review.
const shots = process.env['SCREENSHOTS'];
const shot = async (page: Page, name: string) => {
	if (shots) await page.screenshot({ path: `${shots}/import-${name}.png`, fullPage: true });
};

const type = (slug: string, name: string, fields: unknown[], single = false) => ({
	slug,
	version: 1,
	definition: { name, title_field: 'title', fields, ...(single ? { single: true } : {}) },
	definition_hash: `hash-${slug}`,
	created_at: AT
});
const TITLE = { id: 'title', label: 'Title', type: 'text', required: true, max_chars: 200, multiline: false };
const POSTS = type('posts', 'Posts', [
	TITLE,
	{ id: 'slug', label: 'URL name', type: 'slug', required: true, source: 'title' },
	{ id: 'pub_date', label: 'Published on', type: 'date', required: false },
	{ id: 'tags', label: 'Tags', type: 'list', required: false, of: { type: 'text', max_chars: 40, multiline: false }, max: 10 },
	{ id: 'body', label: 'Body', type: 'markdown', required: false, max_chars: 100_000 }
]);
const PAGES = type('pages', 'Pages', [TITLE, { id: 'body', label: 'Body', type: 'rich_text', required: false }]);
const SETTINGS = type('site', 'Site settings', [TITLE], true);

interface Sent {
	dryRun: boolean;
	kind: string | null;
	key: string | null;
	items: Record<string, unknown>[];
}
interface World {
	types: unknown[];
	/** What the role may do; everything when left out. */
	actions?: string[];
	sent: Sent[];
	/** What an import (not a check) answers, by which import call it is, from 0. */
	apply?: (call: number, items: Record<string, unknown>[]) => 'ok' | 'abort' | { status: number; body: unknown };
	unanswered: string[];
}
const ALL = ['content.read', 'content.create', 'content.update', 'content.delete', 'content.review', 'content.publish', 'schema.change', 'members.manage'];

/** One item's verdict, roughly as the server's: a title is required, and a name the type doesn't
 *  declare is refused by name. Source envelopes are unwrapped as `transfer::map` does. */
function judge(item: Record<string, unknown>, index: number, fields: string[]) {
	const at = `/${index}`;
	let document = item;
	const unmapped: string[] = [];
	if ('_type' in item) {
		document = Object.fromEntries(Object.entries(item).filter(([key]) => !key.startsWith('_')));
		unmapped.push(...Object.keys(item).filter((key) => key.startsWith('_') && key !== '_id' && key !== '_type'));
	}
	const unknown = Object.keys(document).find((key) => !fields.includes(key));
	if (unknown !== undefined) return { item: at, status: 'invalid', refusal: { code: 'unknown_field', pointer: `${at}/${unknown}`, message: `\`${unknown}\` is not allowed here` } };
	if (typeof document['title'] !== 'string') return { item: at, status: 'invalid', refusal: { code: 'missing_field', pointer: `${at}/title`, message: '`Title` is required' } };
	return { item: at, status: 'would_create', entry: `entry-${index}`, ...(unmapped.length > 0 ? { unmapped: unmapped.sort() } : {}) };
}

async function serve(page: Page, w: World) {
	let applies = 0;
	await page.context().route(
		(url) => url.pathname === '/me' || url.pathname.startsWith('/api/'),
		async (route: Route) => {
			const request = route.request();
			const url = new globalThis.URL(request.url());
			const path = url.pathname;
			const ok = (body: unknown, headers: Record<string, string> = {}, status = 200) =>
				route.fulfill({ status, headers: { 'content-type': 'application/json', ...headers }, body: JSON.stringify(body) });
			const permissions = (w.actions ?? ALL).map((action) => ({ action, subject: '*' }));
			if (path === '/me') {
				return ok({
					data: {
						id: 'person-1',
						issuer: 'colophon',
						subject: 'person-1',
						display_name: 'Test editor',
						administrator: false,
						installation: { has_administrator: true },
						projects: [{ id: PROJECT, slug: 'the-draft', name: 'The Draft', role: 'owner', role_name: 'Owner' }]
					}
				});
			}
			if (path === `/api/v1/projects/${PROJECT}`) {
				return ok({ data: { id: PROJECT, slug: 'the-draft', name: 'The Draft', role: 'owner', role_name: 'Owner', permissions, self_review: true } });
			}
			if (!path.startsWith(P)) {
				w.unanswered.push(`${request.method()} ${path}`);
				return ok({ error: { code: 'not_found', message: 'not found', details: { reason: 'not_found' } } }, {}, 404);
			}
			const rest = path.slice(P.length);
			if (rest === '/content-types') return ok({ data: w.types });
			if (rest === '/changesets' || rest === '/migrations') return ok({ data: [] });
			if (rest === '/releases') return ok({ data: [] }, { 'colophon-generation': '0' });
			if (rest === '/events') return route.fulfill({ status: 204 });
			if (/^\/collections\/[a-z]+\/entries$/.test(rest)) return ok({ data: [], next_cursor: null });
			const imported = /^\/collections\/([a-z]+)\/import$/.exec(rest);
			if (imported !== null && request.method() === 'POST') {
				const items = request.postDataJSON() as Record<string, unknown>[];
				const dryRun = url.searchParams.get('dry_run') === 'true';
				w.sent.push({ dryRun, kind: url.searchParams.get('type'), key: request.headers()['idempotency-key'] ?? null, items });
				const fields = ((w.types as (typeof POSTS)[]).find((one) => one.slug === imported[1])?.definition.fields ?? []).map((field) => (field as { id: string }).id);
				if (dryRun) return ok({ data: { dry_run: true, outcomes: items.map((item, index) => judge(item, index, fields)) } });
				const answer = w.apply?.(applies, items) ?? 'ok';
				applies += 1;
				if (answer === 'abort') return route.abort('internetdisconnected');
				if (answer !== 'ok') return ok(answer.body, {}, answer.status);
				return ok({ data: { imported: items.length, outcomes: items.map((_, index) => ({ item: `/${index}`, status: 'created', entry: `entry-${index}` })) } }, {}, 201);
			}
			w.unanswered.push(`${request.method()} ${path}`);
			return ok({ error: { code: 'not_found', message: 'not found', details: { reason: 'not_found' } } }, {}, 404);
		}
	);
}

const world = (overrides: Partial<World> = {}): World => ({ types: [POSTS, PAGES, SETTINGS], sent: [], unanswered: [], ...overrides });
const file = (name: string, text: string | Buffer) => ({ name, mimeType: 'application/octet-stream', buffer: typeof text === 'string' ? Buffer.from(text) : text });
/** Primary buttons on screen: a screen leads with one thing. */
const primaries = (page: Page) => page.locator('main [data-variant="default"]:visible');

async function open(page: Page, w: World) {
	await serve(page, w);
	await page.goto(URL);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Import content');
}

test('the page asks where the content is, offers what it can import and says what it can’t', async ({ page }) => {
	const w = world();
	await open(page, w);
	await expect(page.getByText('Where is your content now?')).toBeVisible();
	await expect(page.getByTestId('import-sources').getByRole('radio')).toHaveCount(5);
	for (const name of ['Strapi', 'Sanity', 'Markdown files', 'Colophon', 'JSON file']) {
		await expect(page.getByRole('radio', { name: new RegExp(`^${name}`) })).toBeVisible();
	}
	const unsupported = page.getByTestId('import-unsupported');
	await expect(unsupported).toContainText("WordPress, Contentful, Ghost and CSV files can't be imported yet");
	await expect(unsupported.getByRole('link')).toHaveAttribute('href', 'https://github.com/akira-shirase-oss/colophon-cms/issues');
	// The sidebar leads here, and says where it is.
	await expect(page.getByTestId('import-link')).toHaveAttribute('data-active', 'true');
	// Nothing to press until a source is chosen.
	await expect(page.getByTestId('import-check')).toHaveCount(0);

	// Each source says how to get the content out, in steps; the choice works from the keyboard.
	await page.getByTestId('source-strapi').focus();
	await page.keyboard.press('Space');
	await expect(page.getByTestId('import-steps')).toContainText('Get your content out of Strapi');
	await expect(page.getByTestId('import-steps').getByRole('listitem')).toHaveCount(3);
	await page.keyboard.press('ArrowDown');
	await expect(page.getByTestId('import-steps')).toContainText('Get your content out of Sanity');
	await page.getByTestId('source-colophon').check();
	await expect(page.getByTestId('import-steps')).toContainText('press Export');
	await shot(page, 'source-chosen');
	// A single type isn't a place for a file of entries.
	await expect(page.getByTestId('import-type').locator('option')).toHaveText(['Choose a content type', 'Posts', 'Pages']);
	expect(await primaries(page).count()).toBeLessThanOrEqual(1);
	expect(await internalTerms(page)).toEqual([]);
	expect((await new AxeBuilder({ page }).analyze()).violations.map((violation) => violation.id)).toEqual([]);
	expect(w.unanswered).toEqual([]);
});

test('Markdown files become drafts: front matter matched to fields, the body in the Markdown field, the URL name from the file', async ({ page }) => {
	const w = world();
	await open(page, w);
	await page.getByTestId('source-markdown').check();
	await page.getByTestId('import-type').selectOption('posts');
	await page.getByTestId('import-file').setInputFiles([
		file('hello-world.md', '---\ntitle: Hello world\npubDate: 2024-05-01T10:00:00Z\ntags: [a, b]\nheroImage: ./hero.jpg\ndraft: false\n---\n\n# Hello\r\n\r\nFirst post.\n'),
		file('Second Post.md', '---\ntitle: Second\nslug: second-one\n---\nMore.\n'),
		file('untitled.md', 'No front matter here.\n'),
		file('broken.md', '---\ntitle: [oops\n---\nBody\n'),
		file('toml.md', '+++\ntitle = "Hugo"\n+++\nBody\n')
	]);
	await expect(page.getByTestId('import-found')).toContainText('3 files read: 3 entries.');
	// What couldn't be read is named, with why, and left out.
	const unread = page.getByTestId('import-file-problems');
	await expect(unread).toContainText("2 files couldn't be read, and are left out");
	await expect(unread).toContainText('broken.md: Its front matter can’t be read');
	await expect(unread).toContainText('toml.md: Its front matter is between +++ lines');

	// The mapping: exact names, then the same name written another way; the rest left out, said plainly.
	const mapping = page.getByTestId('import-mapping');
	const mapped = (name: string) => mapping.locator(`select[data-name="${name}"]`);
	await expect(mapped('title')).toHaveValue('title');
	await expect(mapped('pubDate')).toHaveValue('pub_date');
	await expect(mapped('tags')).toHaveValue('tags');
	await expect(mapped('slug')).toHaveValue('slug');
	await expect(mapped('heroImage')).toHaveValue('');
	await expect(page.getByTestId('import-left-out')).toContainText('2 names match no field and are left out of the import: heroImage, draft.');
	// The body's field is never offered to front matter.
	await expect(mapped('title').locator('option')).toHaveText(['Leave out', 'Title', 'URL name', 'Published on', 'Tags']);
	await expect(mapping).toContainText('The text under the front matter goes into Body.');
	await expect(page.getByLabel('pubDate')).toBeVisible();

	await expect(page.getByTestId('import-check')).toHaveText('Check the files');
	await shot(page, 'markdown-mapped');
	await expect(primaries(page)).toHaveCount(1);
	await page.getByTestId('import-check').click();
	const summary = page.getByTestId('import-summary');
	await expect(summary.getByTestId('will-create')).toHaveText('2');
	await expect(summary.getByTestId('will-update')).toHaveText('0');
	await expect(summary.getByTestId('will-refuse')).toHaveText('1');
	await expect(page.getByTestId('import-problems')).toContainText('untitled.md: Title is required.');
	// The check wrote nothing and needed no key.
	expect(w.sent).toHaveLength(1);
	expect(w.sent[0]).toMatchObject({ dryRun: true, key: null, kind: null });
	// In the files' order by name, each as the document its front matter and body describe.
	expect(w.sent[0]?.items).toEqual([
		{ title: 'Hello world', pub_date: '2024-05-01', tags: ['a', 'b'], body: '# Hello\n\nFirst post.', slug: 'hello-world' },
		{ title: 'Second', slug: 'second-one', body: 'More.' },
		{ body: 'No front matter here.', slug: 'untitled' }
	]);
	expect(await internalTerms(page)).toEqual([]);
	expect((await new AxeBuilder({ page }).analyze()).violations.map((violation) => violation.id)).toEqual([]);

	await shot(page, 'markdown-checked');
	// One primary: the import of what the check accepted. It says drafts, and that nothing is published.
	await expect(primaries(page)).toHaveCount(1);
	await expect(page.getByTestId('import-apply')).toHaveText('Import 2 entries');
	await expect(page.getByText('Nothing is published, and your website doesn')).toBeVisible();
	await page.getByTestId('import-apply').click();
	const done = page.getByTestId('import-done');
	await expect(done).toContainText('Imported 2 entries');
	await expect(done).toContainText('They are drafts. Nothing is published until you publish it.');
	await expect(page.getByTestId('import-not-all')).toContainText('1 entry from these files was not imported');
	await shot(page, 'markdown-done');
	await expect(page.getByTestId('import-open')).toHaveAttribute('href', `/studio/projects/${PROJECT}/posts`);
	await expect(primaries(page)).toHaveCount(1);
	// Only the two the check accepted were sent, with a key.
	expect(w.sent).toHaveLength(2);
	expect(w.sent[1]?.dryRun).toBe(false);
	expect(w.sent[1]?.key).toMatch(/^[0-9a-f]{32}$/);
	expect(w.sent[1]?.items.map((item) => item['title'])).toEqual(['Hello world', 'Second']);

	// What's left is only the one that couldn't be imported: matching nothing more, it still can't.
	await page.getByTestId('import-check-rest').click();
	await expect(page.getByTestId('import-summary').getByTestId('will-refuse')).toHaveText('1');
	expect(w.sent[2]?.items).toEqual([{ body: 'No front matter here.', slug: 'untitled' }]);
	await expect(page.getByTestId('import-apply')).toHaveCount(0);
	expect(w.unanswered).toEqual([]);
});

test('a name can be pointed at another field or left out, and the check starts over', async ({ page }) => {
	const w = world();
	await open(page, w);
	await page.getByTestId('source-json').check();
	await page.getByTestId('import-type').selectOption('posts');
	await page.getByTestId('import-file').setInputFiles(file('posts.json', JSON.stringify([{ Title: 'One', headline: 'The first', published: '2024-01-02T03:04:05Z' }])));
	const mapped = (name: string) => page.getByTestId('import-mapping').locator(`select[data-name="${name}"]`);
	await expect(mapped('Title')).toHaveValue('title');
	await expect(mapped('published')).toHaveValue('');
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('will-create')).toHaveText('1');
	// Changing the mapping makes the check about something else: it is taken away, not left to mislead.
	await mapped('published').selectOption('pub_date');
	await expect(page.getByTestId('import-summary')).toHaveCount(0);
	await mapped('Title').selectOption('');
	await mapped('headline').selectOption('title');
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('will-create')).toHaveText('1');
	expect(w.sent.at(-1)?.items).toEqual([{ title: 'The first', pub_date: '2024-01-02' }]);
	// A required field nothing fills is said before the check.
	await mapped('headline').selectOption('');
	await expect(page.getByTestId('import-unfilled')).toContainText('Nothing here fills Title and URL name, which are required.');

	// Opening the chooser starts over, so the same file, fixed and chosen again, is read again.
	const [chooser] = await Promise.all([page.waitForEvent('filechooser'), page.getByTestId('import-file').click()]);
	await expect(page.getByTestId('import-mapping')).toHaveCount(0);
	await chooser.setFiles(file('posts.json', JSON.stringify([{ title: 'Fixed' }, { title: 'Too' }])));
	await expect(page.getByTestId('import-found')).toContainText('1 file read: 2 entries.');
	await expect(mapped('title')).toHaveValue('title');
	await expect(mapped('headline')).toHaveCount(0);
});

test('a content type with no Markdown field says so and stops', async ({ page }) => {
	const w = world();
	await open(page, w);
	await page.getByTestId('source-markdown').check();
	await page.getByTestId('import-type').selectOption('pages');
	await expect(page.getByTestId('import-no-markdown-field')).toContainText('Pages has no Markdown field');
	await expect(page.getByTestId('import-no-markdown-field').getByRole('link')).toHaveAttribute('href', `/studio/projects/${PROJECT}/settings/types/pages`);
	await expect(page.getByTestId('import-file')).toHaveCount(0);
	await expect(page.getByTestId('import-check')).toHaveCount(0);
	// With a Markdown field, the files can be chosen.
	await page.getByTestId('import-type').selectOption('posts');
	await expect(page.getByTestId('import-file')).toBeVisible();
});

test('a mixed Sanity export is narrowed to one kind; a file that isn’t what its source says is turned away', async ({ page }) => {
	const w = world();
	await open(page, w);
	await page.getByTestId('source-sanity').check();
	await page.getByTestId('import-type').selectOption('posts');

	// An archive still needs unpacking.
	await page.getByTestId('import-file').setInputFiles(file('production.tar.gz', Buffer.from([0x1f, 0x8b, 8, 0, 0, 0, 0, 0])));
	await expect(page.getByTestId('import-file-problems')).toContainText('production.tar.gz: This is an archive, not the file of entries. Unpack it first');
	await expect(page.getByTestId('import-check')).toBeDisabled();

	// Entries from Strapi, chosen as Sanity.
	await page.getByTestId('import-file').setInputFiles(file('articles.json', JSON.stringify({ data: [{ id: 1, attributes: { title: 'A' } }], meta: { pagination: { page: 1 } } })));
	await expect(page.getByTestId('import-unusable')).toContainText('This file looks like it came from Strapi, not Sanity. Choose Strapi above.');
	await expect(page.getByTestId('import-check')).toBeDisabled();

	// The real thing: three kinds, one document a line.
	const lines = [
		{ _id: 'p1', _type: 'post', _rev: 'r1', _createdAt: '2024-05-05', title: 'First' },
		{ _id: 'a1', _type: 'author', name: 'Ada' },
		{ _id: 'p2', _type: 'post', _rev: 'r2', title: 'Second' },
		{ _id: 'i1', _type: 'sanity.imageAsset', url: 'https://cdn.example.com/a.png' },
		{ _id: 'i2', _type: 'sanity.imageAsset', url: 'https://cdn.example.com/b.png' },
		{ _id: 'i3', _type: 'sanity.imageAsset', url: 'https://cdn.example.com/c.png' }
	];
	await page.getByTestId('import-file').setInputFiles(file('data.ndjson', lines.map((line) => JSON.stringify(line)).join('\n')));
	await expect(page.getByTestId('import-found')).toContainText('1 file read: 6 entries.');
	const kind = page.getByTestId('import-kind');
	// The kind named like the content type, not the most common one.
	await expect(kind).toHaveValue('post');
	await expect(kind.locator('option')).toHaveText(['sanity.imageAsset (3)', 'post (2)', 'author (1)']);
	await expect(page.getByText('The 4 entries of other kinds are left out')).toBeVisible();
	await shot(page, 'sanity-kinds');
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('will-create')).toHaveText('2');
	await expect(page.getByTestId('import-summary')).toContainText('Set aside, not stored: _createdAt, _rev.');
	// Only that kind's documents travel, as they are, with the kind named for the server.
	expect(w.sent.at(-1)).toMatchObject({ dryRun: true, kind: 'post' });
	expect(w.sent.at(-1)?.items).toEqual([lines[0], lines[2]]);

	// Another kind is another check.
	await kind.selectOption('author');
	await expect(page.getByTestId('import-summary')).toHaveCount(0);
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('will-refuse')).toHaveText('1');
	await expect(page.getByTestId('import-problems')).toContainText('It has “name”, and this content type has no field with that name.');
	await expect(page.getByTestId('import-apply')).toHaveCount(0);
	expect(await internalTerms(page)).toEqual([]);
});

test('a long file goes in batches, and a refused batch is reported as a part, with what landed and what was left', async ({ page }) => {
	const entries = Array.from({ length: 250 }, (_, index) => ({ title: `Post ${index + 1}`, slug: `post-${index + 1}` }));
	const w = world({
		// The second batch is refused whole for its eighth entry, as the server refuses a batch.
		apply: (call) =>
			call === 1
				? { status: 422, body: { error: { code: 'slug_taken', message: 'another entry already uses this slug', details: { reason: 'slug_taken', pointer: '/7/slug' } } } }
				: 'ok'
	});
	await open(page, w);
	await page.getByTestId('source-colophon').check();
	await page.getByTestId('import-type').selectOption('posts');
	await page.getByTestId('import-file').setInputFiles(file('posts.jsonl', entries.map((entry) => JSON.stringify(entry)).join('\n')));
	await expect(page.getByTestId('import-found')).toContainText('1 file read: 250 entries.');
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('will-create')).toHaveText('250');
	expect(w.sent.map((sent) => [sent.dryRun, sent.items.length])).toEqual([[true, 100], [true, 100], [true, 50]]);
	// The check's blind spot across batches is said.
	await expect(page.getByTestId('import-summary')).toContainText("the check can't see two entries in different batches that clash");

	await page.getByTestId('import-apply').click();
	const stopped = page.getByTestId('import-stopped');
	await expect(stopped).toContainText('Imported 100 of 250 entries');
	await expect(stopped).toContainText('Batch 2 of 3 (entries 101 to 200) was refused, and none of that batch was imported.');
	await expect(stopped).toContainText('The 100 entries before it are in, as drafts. The 50 entries after it were not sent.');
	// The entry and the field the refusal names.
	await expect(stopped).toContainText('Post 108 (Entry 108): Another entry already uses this URL name.');
	await expect(page.getByTestId('import-done')).toHaveCount(0);
	await shot(page, 'stopped');
	// The stopped run reads in dark too.
	await page.emulateMedia({ colorScheme: 'dark' });
	expect((await new AxeBuilder({ page }).analyze()).violations.map((violation) => violation.id)).toEqual([]);
	await shot(page, 'stopped-dark');
	await page.emulateMedia({ colorScheme: 'light' });
	// The third batch was never sent.
	expect(w.sent.filter((sent) => !sent.dryRun).map((sent) => sent.items.length)).toEqual([100, 100]);
	expect(await internalTerms(page)).toEqual([]);
	await expect(primaries(page)).toHaveCount(1);

	// What's left is checked again: the 150 that didn't land, never the 100 that did.
	await page.getByTestId('import-check-rest').click();
	await expect(page.getByTestId('will-create')).toHaveText('150');
	await expect(page.getByTestId('import-found')).toContainText('100 entries from here are already imported and left out.');
	const again = w.sent.slice(-2);
	expect(again.map((sent) => [sent.dryRun, sent.items.length])).toEqual([[true, 100], [true, 50]]);
	expect(again[0]?.items[0]).toEqual({ title: 'Post 101', slug: 'post-101' });
	await page.getByTestId('import-apply').click();
	await expect(page.getByTestId('import-done')).toContainText('Imported 150 entries');
});

test('a batch that gets no answer is unknown, and trying again sends the same batch with the same key', async ({ page }) => {
	test.slow();
	const entries = Array.from({ length: 120 }, (_, index) => ({ title: `Post ${index + 1}` }));
	// The second batch gets no answer three times running (the page tries three times), then lands.
	const w = world({ apply: (call) => (call >= 1 && call <= 3 ? 'abort' : 'ok') });
	await open(page, w);
	await page.getByTestId('source-json').check();
	await page.getByTestId('import-type').selectOption('posts');
	await page.getByTestId('import-file').setInputFiles(file('posts.json', JSON.stringify(entries)));
	await page.getByTestId('import-check').click();
	await page.getByTestId('import-apply').click();
	await expect(page.getByTestId('import-progress')).toBeVisible();
	const stopped = page.getByTestId('import-stopped');
	await expect(stopped).toContainText('Imported 100 of 120 entries so far', { timeout: 20_000 });
	await expect(stopped).toContainText('Batch 2 of 2 (entries 101 to 120) got no answer, so it isn’t known whether those were imported.');
	await expect(stopped).toContainText('Try again: the same batch is never added twice.');
	await page.getByTestId('import-retry').click();
	await expect(page.getByTestId('import-done')).toContainText('Imported 120 entries');
	const applied = w.sent.filter((sent) => !sent.dryRun);
	expect(applied.map((sent) => sent.items.length)).toEqual([100, 20, 20, 20, 20]);
	// Every attempt at the second batch carried one key, and not the first batch's.
	expect(new Set(applied.slice(1).map((sent) => sent.key)).size).toBe(1);
	expect(applied[1]?.key).not.toBe(applied[0]?.key);
});

test('a project with no content types, and a role that can’t create entries, each say what to do', async ({ page }) => {
	const empty = world({ types: [SETTINGS] });
	await open(page, empty);
	const none = page.getByTestId('import-no-types');
	await expect(none).toContainText('No content types yet');
	await expect(none.getByRole('link', { name: 'Make a content type' })).toHaveAttribute('href', `/studio/projects/${PROJECT}/settings/types`);
	await expect(page.getByTestId('import-sources')).toHaveCount(0);

	// A reader: the page says why not, and the sidebar doesn't offer it.
	await page.context().unrouteAll();
	const reader = world({ actions: ['content.read'] });
	await open(page, reader);
	await expect(page.getByTestId('import-not-allowed')).toContainText("Your role can't create entries in this project, so it can't import any.");
	await expect(page.getByTestId('import-link')).toHaveCount(0);
	await expect(page.getByTestId('import-sources')).toHaveCount(0);
});

test('the schema page is named for what it is, and points people moving content to Import content', async ({ page }) => {
	const w = world();
	await serve(page, w);
	await page.goto(`/studio/projects/${PROJECT}/migrations`);
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Content type changes');
	await expect(page).toHaveTitle('Content type changes · Colophon Studio');
	await expect(page.getByTestId('migrations-link')).toHaveText('Content type changes');
	const note = page.getByTestId('migrations-import-note');
	await expect(note).toContainText('Bringing content in from another CMS is done on Import content.');
	await note.getByRole('link', { name: 'Import content' }).click();
	await expect(page).toHaveURL(new RegExp(`/studio/projects/${PROJECT}/import$`));
	await expect(page.getByRole('heading', { level: 1 })).toHaveText('Import content');
	expect(w.unanswered).toEqual([]);
});

test('at phone width the page, its mapping and its summary fit the screen', async ({ page }) => {
	await page.setViewportSize({ width: 390, height: 844 });
	const w = world();
	await open(page, w);
	await page.getByTestId('source-markdown').check();
	await page.getByTestId('import-type').selectOption('posts');
	await page.getByTestId('import-file').setInputFiles([
		file('a-rather-long-file-name-for-a-post-about-something-or-other-2024.md', '---\ntitle: Hello\na_very_long_front_matter_name_that_goes_on: x\n---\nBody\n'),
		file('untitled.md', 'No title.\n')
	]);
	await expect(page.getByTestId('import-mapping')).toBeVisible();
	const fits = () => page.evaluate(() => document.documentElement.scrollWidth <= window.innerWidth);
	expect(await fits()).toBe(true);
	await page.getByTestId('import-check').click();
	await expect(page.getByTestId('import-problems')).toBeVisible();
	expect(await fits()).toBe(true);
	await shot(page, 'phone-checked');
	await page.emulateMedia({ colorScheme: 'dark' });
	expect((await new AxeBuilder({ page }).analyze()).violations.map((violation) => violation.id)).toEqual([]);
	expect((await new AxeBuilder({ page }).analyze()).violations.map((violation) => violation.id)).toEqual([]);
});
