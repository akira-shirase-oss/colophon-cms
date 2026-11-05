// Seeds the five Astro demo sites (apps/sites/README.md; D-133): signs in as the development
// realm's dev-owner the way a browser does, creates one `demo-sites` project holding five
// collections (`posts`, `guides`, `recipes`, `stories` and `photos`), uploads generated PNG
// photographs through the real quarantine and verify pipeline, writes a few dozen entries, and
// publishes them all as release 1 through the changeset flow, so the sites prove published
// content, never drafts. Finally it writes `apps/sites/.colophon.json`, the record the sites
// read at build time, and prints the project id and the release's generation.
//
//   node scripts/seed-demo-sites.mts [origin]
//
// `origin` defaults to the development server on http://localhost:58000. Seeding creates a new
// project every run, so running it twice is safe: the newest run's project is what the sites
// build from.

import zlib from 'node:zlib';
import { writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { dirname, resolve } from 'node:path';

const [origin = 'http://localhost:58000'] = process.argv.slice(2);
const HERE = dirname(fileURLToPath(import.meta.url));
const RECORD = resolve(HERE, '../apps/sites/.colophon.json');

/* ------------------------------------------------------------- the session */

const cookies = new Map<string, string>();

function remember(response: Response): void {
	for (const line of response.headers.getSetCookie()) {
		const [pair = ''] = line.split(';');
		const at = pair.indexOf('=');
		if (at > 0) cookies.set(pair.slice(0, at).trim(), pair.slice(at + 1).trim());
	}
}

function cookieHeader(): string {
	return [...cookies].map(([name, value]) => `${name}=${value}`).join('; ');
}

/** Follows redirects by hand, keeping cookies for every host on the way. */
async function visit(url: string, init: RequestInit = {}): Promise<Response> {
	let next = url;
	let options: RequestInit = init;
	for (let hop = 0; hop < 10; hop += 1) {
		const response = await fetch(next, {
			...options,
			redirect: 'manual',
			headers: { ...(options.headers ?? {}), cookie: cookieHeader() }
		});
		remember(response);
		const location = response.headers.get('location');
		if (response.status < 300 || response.status >= 400 || location === null) return response;
		next = new URL(location, next).toString();
		options = {};
	}
	throw new Error('too many redirects');
}

async function signIn(): Promise<void> {
	const form = await visit(`${origin}/auth/login?return_to=/me`);
	const action = /<form[^>]*id="kc-form-login"[^>]*action="([^"]+)"/.exec(await form.text())?.[1];
	if (action === undefined) throw new Error('no Keycloak sign-in form');
	const signedIn = await visit(action.replaceAll('&amp;', '&'), {
		method: 'POST',
		headers: { 'content-type': 'application/x-www-form-urlencoded' },
		body: new URLSearchParams({ username: 'dev-owner', password: 'colophon-dev' })
	});
	if (signedIn.status !== 200) throw new Error(`sign-in failed: HTTP ${signedIn.status}`);
}

async function api<T>(method: string, path: string, body?: unknown, headers: Record<string, string> = {}): Promise<T> {
	const response = await fetch(`${origin}${path}`, {
		method,
		headers: {
			cookie: cookieHeader(),
			origin,
			'x-csrf-token': cookies.get('colophon_csrf') ?? '',
			...(body === undefined ? {} : { 'content-type': 'application/json' }),
			...headers
		},
		...(body === undefined ? {} : { body: JSON.stringify(body) })
	});
	const text = await response.text();
	const parsed = text === '' ? {} : (JSON.parse(text) as { data?: T });
	if (response.status >= 400) throw new Error(`${method} ${path}: HTTP ${response.status} ${text}`);
	return parsed.data as T;
}

const key = () => crypto.randomUUID().replaceAll('-', '');

/* ------------------------------------------------------------- rich text */

type Block = Record<string, unknown>;

const para = (...texts: (string | Record<string, unknown>)[]): Block => ({
	type: 'paragraph',
	content: texts.map((text) => (typeof text === 'string' ? { type: 'text', text } : text))
});
const strong = (text: string) => ({ type: 'text', text, marks: [{ type: 'bold' }] });
const em = (text: string) => ({ type: 'text', text, marks: [{ type: 'italic' }] });
const link = (text: string, href: string) => ({ type: 'text', text, marks: [{ type: 'link', attrs: { href } }] });
const h2 = (text: string): Block => ({ type: 'heading', attrs: { level: 2 }, content: [{ type: 'text', text }] });
const bullets = (...items: string[]): Block => ({
	type: 'bulletList',
	content: items.map((text) => ({ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] }))
});
const numbered = (...items: string[]): Block => ({
	type: 'orderedList',
	content: items.map((text) => ({ type: 'listItem', content: [{ type: 'paragraph', content: [{ type: 'text', text }] }] }))
});
const quote = (...blocks: Block[]): Block => ({ type: 'blockquote', content: blocks });
const callout = (tone: 'note' | 'tip' | 'warning', ...paragraphs: string[]): Block => ({
	type: 'callout',
	attrs: { tone },
	content: paragraphs.map((text) => ({ type: 'paragraph', content: [{ type: 'text', text }] }))
});
const code = (text: string, language = 'text'): Block => ({ type: 'codeBlock', attrs: { language }, content: [{ type: 'text', text }] });
const rule = (): Block => ({ type: 'horizontalRule' });

function richText(...blocks: Block[]): unknown {
	return { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: blocks } };
}

/* ------------------------------------------------------- generated photos */

// The portfolio's photographs are drawn, not fetched: small PNG encoders produce real,
// different pictures so the upload, verification and rendition pipeline has honest work to do.

const CRC_TABLE = (() => {
	const table = new Uint32Array(256);
	for (let n = 0; n < 256; n += 1) {
		let c = n;
		for (let k = 0; k < 8; k += 1) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
		table[n] = c >>> 0;
	}
	return table;
})();

function crc32(bytes: Buffer): number {
	let c = 0xffffffff;
	for (const byte of bytes) c = CRC_TABLE[(c ^ byte) & 0xff]! ^ (c >>> 8);
	return (c ^ 0xffffffff) >>> 0;
}

function chunk(kind: string, data: Buffer): Buffer {
	const length = Buffer.alloc(4);
	length.writeUInt32BE(data.length);
	const body = Buffer.concat([Buffer.from(kind, 'ascii'), data]);
	const crc = Buffer.alloc(4);
	crc.writeUInt32BE(crc32(body));
	return Buffer.concat([length, body, crc]);
}

type Rgb = [number, number, number];

function png(width: number, height: number, pixel: (x: number, y: number) => Rgb): Buffer {
	const header = Buffer.alloc(13);
	header.writeUInt32BE(width, 0);
	header.writeUInt32BE(height, 4);
	header[8] = 8; // bit depth
	header[9] = 2; // truecolour
	const raw = Buffer.alloc((width * 3 + 1) * height);
	for (let y = 0; y < height; y += 1) {
		const row = y * (width * 3 + 1);
		raw[row] = 0; // filter: none
		for (let x = 0; x < width; x += 1) {
			const [r, g, b] = pixel(x, y);
			const at = row + 1 + x * 3;
			raw[at] = r;
			raw[at + 1] = g;
			raw[at + 2] = b;
		}
	}
	return Buffer.concat([
		Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
		chunk('IHDR', header),
		chunk('IDAT', zlib.deflateSync(raw, { level: 9 })),
		chunk('IEND', Buffer.alloc(0))
	]);
}

const mix = (a: Rgb, b: Rgb, t: number): Rgb => [
	Math.round(a[0] + (b[0] - a[0]) * t),
	Math.round(a[1] + (b[1] - a[1]) * t),
	Math.round(a[2] + (b[2] - a[2]) * t)
];

/** A sky gradient with a low sun (or moon) and horizontal silhouette bands. */
function scene(top: Rgb, horizon: Rgb, sun: Rgb, bands: Rgb[], sunX = 0.68, sunY = 0.58, sunR = 0.09): (x: number, y: number) => Rgb {
	return (x, y) => {
		const w = 1600;
		const h = 1000;
		const ny = y / h;
		let colour = mix(top, horizon, Math.min(1, ny / 0.72));
		const dx = (x / w - sunX) * (w / h);
		const dy = y / h - sunY;
		const d = Math.sqrt(dx * dx + dy * dy);
		if (d < sunR) colour = mix(sun, colour, d / sunR);
		else if (d < sunR * 1.7) colour = mix(colour, sun, 0.25 * (1 - (d - sunR) / (sunR * 0.7)));
		for (const [at, rgb] of bands.map<[number, Rgb]>((rgb, i) => [0.62 + i * 0.09, rgb])) {
			const edge = at + 0.012 * Math.sin(x / 97 + at * 40) + 0.008 * Math.sin(x / 31);
			if (ny > edge) colour = rgb;
		}
		return colour;
	};
}

const W = 1600;
const H = 1000;

const PHOTOS: Array<{ file: string; pixel: (x: number, y: number) => Rgb }> = [
	{ file: 'overpass-dusk.png', pixel: scene([38, 32, 54], [214, 106, 47], [255, 190, 92], [[30, 28, 40], [22, 20, 30], [14, 13, 20]], 0.7, 0.5, 0.08) },
	{ file: 'night-bus-depot.png', pixel: scene([16, 24, 40], [96, 120, 160], [220, 230, 245], [[20, 26, 40], [14, 18, 30], [10, 13, 22]], 0.3, 0.38, 0.06) },
	{ file: 'fellside-rain.png', pixel: scene([96, 110, 118], [168, 176, 168], [210, 214, 206], [[62, 74, 66], [44, 56, 50], [30, 40, 36]], 0.5, 0.3, 0.12) },
	{ file: 'salt-marsh.png', pixel: scene([150, 170, 190], [230, 210, 180], [250, 236, 200], [[120, 124, 108], [88, 96, 84], [60, 68, 60]], 0.42, 0.52, 0.1) },
	{ file: 'chess-tables.png', pixel: scene([52, 40, 34], [190, 150, 96], [255, 226, 160], [[46, 34, 28], [34, 25, 20], [24, 18, 14]], 0.62, 0.44, 0.1) },
	{ file: 'market-porters.png', pixel: scene([70, 60, 70], [200, 130, 90], [255, 208, 140], [[54, 44, 52], [40, 32, 38], [28, 22, 27]], 0.5, 0.42, 0.09) },
	{ file: 'darkroom-tools.png', pixel: scene([40, 22, 22], [150, 60, 46], [255, 140, 90], [[34, 18, 18], [26, 14, 14], [18, 10, 10]], 0.55, 0.5, 0.11) },
	{ file: 'winter-apples.png', pixel: scene([96, 100, 110], [190, 180, 170], [240, 230, 215], [[80, 76, 84], [58, 54, 62], [40, 37, 44]], 0.4, 0.45, 0.1) },
	{ file: 'tidal-flats.png', pixel: scene([110, 140, 160], [220, 200, 170], [248, 230, 190], [[90, 100, 110], [70, 82, 92], [52, 64, 74]], 0.6, 0.5, 0.09) },
	{ file: 'glasshouse.png', pixel: scene([60, 90, 80], [170, 200, 160], [235, 245, 200], [[50, 70, 64], [36, 54, 48], [26, 40, 36]], 0.5, 0.4, 0.1) }
];

/* ----------------------------------------------------------- content types */

const COLLECTIONS: Record<string, { slug: string; definition: unknown }> = {
	posts: {
		slug: 'posts',
		definition: {
			name: 'Blog posts',
			title_field: 'title',
			fields: [
				{ id: 'title', label: 'Title', type: 'text', required: true, index: true },
				{ id: 'slug', label: 'URL name', type: 'slug', source: 'title' },
				{ id: 'excerpt', label: 'Excerpt', type: 'text', multiline: true, max_chars: 400 },
				{ id: 'body', label: 'Body', type: 'rich_text', required: true },
				{ id: 'author', label: 'Author', type: 'text' },
				{ id: 'published_on', label: 'Published on', type: 'date', index: true },
				{ id: 'tags', label: 'Tags', type: 'list', of: { type: 'text', max_chars: 40 }, max: 6 }
			]
		}
	},
	guides: {
		slug: 'guides',
		definition: {
			name: 'Guides',
			title_field: 'title',
			fields: [
				{ id: 'title', label: 'Title', type: 'text', required: true, index: true },
				{ id: 'slug', label: 'URL name', type: 'slug', source: 'title' },
				{ id: 'summary', label: 'Summary', type: 'text', multiline: true, max_chars: 300 },
				{
					id: 'section',
					label: 'Section',
					type: 'enum',
					values: ['Getting started', 'Guides', 'Reference', 'Advanced'],
					index: true
				},
				{ id: 'position', label: 'Position', type: 'number', integer: true, min: 0, index: true },
				{ id: 'body', label: 'Body', type: 'rich_text', required: true }
			]
		}
	},
	recipes: {
		slug: 'recipes',
		definition: {
			name: 'Recipes',
			title_field: 'title',
			fields: [
				{ id: 'title', label: 'Title', type: 'text', required: true, index: true },
				{ id: 'slug', label: 'URL name', type: 'slug', source: 'title' },
				{ id: 'intro', label: 'Intro', type: 'text', multiline: true, max_chars: 300 },
				{
					id: 'category',
					label: 'Category',
					type: 'enum',
					values: ['Mains', 'Baking', 'Soups and salads', 'Puddings'],
					index: true
				},
				{ id: 'difficulty', label: 'Difficulty', type: 'enum', values: ['Easy', 'Medium', 'A proper project'] },
				{ id: 'prep_minutes', label: 'Prep (minutes)', type: 'number', integer: true, min: 0 },
				{ id: 'cook_minutes', label: 'Cook (minutes)', type: 'number', integer: true, min: 0 },
				{ id: 'servings', label: 'Servings', type: 'number', integer: true, min: 1 },
				{
					id: 'ingredients',
					label: 'Ingredients',
					type: 'list',
					of: {
						type: 'object',
						fields: [
							{ id: 'item', label: 'Item', type: 'text', required: true },
							{ id: 'amount', label: 'Amount', type: 'text' }
						]
					},
					max: 40
				},
				{ id: 'steps', label: 'Steps', type: 'list', of: { type: 'text', multiline: true, max_chars: 500 }, max: 30 },
				{ id: 'notes', label: 'Cook’s notes', type: 'rich_text' }
			]
		}
	},
	stories: {
		slug: 'stories',
		definition: {
			name: 'Stories',
			title_field: 'headline',
			fields: [
				{ id: 'headline', label: 'Headline', type: 'text', required: true, index: true },
				{ id: 'slug', label: 'URL name', type: 'slug', source: 'headline' },
				{
					id: 'category',
					label: 'Category',
					type: 'enum',
					values: ['World', 'Technology', 'Culture', 'Science', 'Sport'],
					index: true
				},
				{ id: 'dateline', label: 'Dateline', type: 'date', index: true },
				{ id: 'byline', label: 'Byline', type: 'text' },
				{ id: 'lede', label: 'Lede', type: 'text', multiline: true, max_chars: 400 },
				{ id: 'body', label: 'Body', type: 'rich_text', required: true },
				{ id: 'featured', label: 'Featured', type: 'boolean', index: true }
			]
		}
	},
	photos: {
		slug: 'photos',
		definition: {
			name: 'Photographs',
			title_field: 'title',
			fields: [
				{ id: 'title', label: 'Title', type: 'text', required: true, index: true },
				{ id: 'slug', label: 'URL name', type: 'slug', source: 'title' },
				{ id: 'photo', label: 'Photograph', type: 'asset', many: false, required: true },
				{ id: 'caption', label: 'Caption', type: 'text', max_chars: 300 },
				{ id: 'location', label: 'Location', type: 'text' },
				{ id: 'taken_on', label: 'Taken on', type: 'date' },
				{
					id: 'series',
					label: 'Series',
					type: 'enum',
					values: ['Urban', 'Landscape', 'People', 'Still life'],
					index: true
				}
			]
		}
	}
};

/* ---------------------------------------------------------------- entries */

interface Seed {
	collection: string;
	content: Record<string, unknown>;
	/** The `?visibility=` it publishes under — `public` when absent (D-137). */
	visibility?: string;
}

/** A seed published behind a gate: `role:<name>` or `authenticated`. */
function gate(seed: Seed, visibility: string): Seed {
	seed.visibility = visibility;
	return seed;
}

function post(title: string, slug: string, author: string, publishedOn: string, tags: string[], excerpt: string, ...body: Block[]): Seed {
	return { collection: 'posts', content: { title, slug, excerpt, body: richText(...body), author, published_on: publishedOn, tags } };
}

function guide(title: string, slug: string, section: string, position: number, summary: string, ...body: Block[]): Seed {
	return { collection: 'guides', content: { title, slug, summary, section, position, body: richText(...body) } };
}

function story(headline: string, slug: string, category: string, dateline: string, byline: string, lede: string, featured: boolean, ...body: Block[]): Seed {
	return { collection: 'stories', content: { headline, slug, category, dateline, byline, lede, body: richText(...body), featured } };
}

const SEEDS: Seed[] = [
	/* The Draft, posts */
	post(
		'Why every publish deserves a receipt',
		'every-publish-deserves-a-receipt',
		'Mara Ellison',
		'2026-10-06',
		['releases', 'editing'],
		'A publish button that only promises is a spinner, not a system. Here is what proof of publish actually looks like.',
		para('Most CMSs answer a publish with a spinner and, eventually, a green tick. The tick means ', em('the request finished'), ', not that the site changed.'),
		para('A receipt is different: it names the release, the destinations it was announced to, and the probes that went and looked.'),
		h2('What a receipt proves'),
		bullets('The release committed, as a numbered generation', 'Each destination was told, with an idempotency key', 'A probe saw the generation marker on the live page'),
		callout('tip', 'Read the Colophon-Generation header on any delivery response: it is the release your content came from.'),
		para('Proof beats promises. A green tick is a mood; a receipt is evidence.')
	),
	post(
		'The changelog is a contract',
		'changelog-is-a-contract',
		'Mara Ellison',
		'2026-10-08',
		['craft'],
		'Nobody reads changelogs until the upgrade breaks at 2am. Write them like the on-call engineer will.',
		para('A changelog entry is a promise to a stranger: ', strong('this is what will be different for you'), '. Not a commit list, not a marketing line.'),
		quote(para('Write the entry for the person upgrading at 2am, not the person who wrote the code.')),
		para('If a change needs a migration step, the entry says so first. Everything else is commentary.')
	),
	post(
		'Drafts are cheap, publish is forever',
		'drafts-are-cheap-publish-is-forever',
		'Tomas Reyes',
		'2026-10-11',
		['workflow', 'editing'],
		'Insert-only revisions mean a draft can be rewritten a hundred times and still show its history. Publish is the point where history becomes a release.',
		para('A draft is a scratch space. Colophon keeps every revision anyway, because the cost of keeping them is small and the cost of losing one is a lost afternoon.'),
		para('Publishing is different: the release pins exact revisions, and the pin is what readers get.'),
		h2('The line that matters'),
		para('Drafts move. Releases do not. That single asymmetry is what makes a publish reviewable.')
	),
	post(
		'What a generation number tells you',
		'what-a-generation-tells-you',
		'Tomas Reyes',
		'2026-10-14',
		['releases'],
		'Release 7 always means the same content. That one property, immutability, is what makes rollbacks honest.',
		para('Ask a numbered release what it contains and the answer never changes. Generation numbers are identities, not versions.'),
		para('Rollback, then, is not "go back to 6": it is a new generation whose content happens to match an older one.'),
		callout('note', 'Generations only move forward. A stale worker can never promote an older one.')
	),
	post(
		'Kill the worker, keep the data',
		'kill-the-worker-keep-the-data',
		'Priya Nair',
		'2026-10-17',
		['engineering'],
		'The crash tests kill the delivery worker mid-POST on a loop. Nothing is lost and nothing happens twice; here is why.',
		para('Every external effect goes through an outbox row with an idempotency key. A killed job is claimed again under a new lease and its key is seen as already applied.'),
		h2('The invariant'),
		bullets('No transaction spans an external call', 'Retried deliveries reuse one key', 'A deduplicating receiver applies exactly one effect'),
		code('apply(key) if key not in applied:\n    applied.add(key)\n    really_apply()', 'text')
	),
	post(
		'Reviewed by hash, not by vibes',
		'reviewed-by-hash-not-vibes',
		'Priya Nair',
		'2026-10-20',
		['releases', 'workflow'],
		'A Colophon review approves a changeset hash. Edit anything afterwards and the approval no longer applies.',
		para('"Looks good to me" attached to a moving target is a meaningless approval. The review binds to the hash of exactly what was shown.'),
		para('If the content changes after approval, the hash changes, and the publish refuses until someone approves the new hash.'),
		quote(para('Approving a diff you have not seen is not review; it is delegation.'))
	),
	post(
		'Slug discipline for busy editors',
		'slug-discipline-for-busy-editors',
		'Mara Ellison',
		'2026-10-23',
		['craft', 'editing'],
		'Slugs are URLs, and URLs are promises. A short house rule keeps them stable without slowing anyone down.',
		para('The slug is set once, at draft time, and then left alone unless someone is prepared to own the redirect.'),
		numbered('Write the title first, derive the slug once', 'Never edit a slug after publish without a redirect', 'Keep slugs under five words; drop articles'),
		para('Boring rules, followed always, beat clever rules followed sometimes.')
	),
	post(
		'The quiet power of a preview deploy',
		'quiet-power-of-preview-deploy',
		'Tomas Reyes',
		'2026-10-26',
		['workflow'],
		'Preview is where bad content goes to be caught. A fifteen-minute capability and a noindex header are all it takes.',
		para('The preview origin renders draft content under a capability that expires. It is noindex, no-store and locked to the one project.'),
		para('An editor who can see the real page stops guessing. Review comments get shorter and better.')
	),
	post(
		'Style sheets are infrastructure too',
		'style-sheets-are-infrastructure',
		'Priya Nair',
		'2026-10-29',
		['craft'],
		'The fastest way to make a publication look careless is an inconsistent em rule. Centralise the choices once.',
		para('A house style sheet is a dependency of every article. Treat it like code: reviewed, versioned, and enforced where it is cheap to enforce.'),
		bullets('One spelling dictionary, not five', 'Numbers as words below ten, numerals above', 'An agreed name for every recurring thing')
	),
	post(
		'Time travel is a debugging tool',
		'time-travel-is-a-debugging-tool',
		'Tomas Reyes',
		'2026-11-01',
		['engineering', 'releases'],
		'"What did the page say last Tuesday?" is a support question with a one-line answer when every release is pinned.',
		para('Add ?generation= to a delivery read and the answer is the release exactly as it was, including which entries did not exist yet.'),
		para('The same pin is what makes a cached page safe to serve: the key names the generation, so a stale fill can never poison a newer release.')
	),
	/* members-only — published role:member; the blog's /members/ pages read them with a live audience session (D-137) */
	gate(
		post(
			'The members’ editing notes, October',
			'members-editing-notes-october',
			'June Okafor',
			'2026-10-30',
			['members', 'craft'],
			'What we cut this month and why: the arguments that never make it into the piece, kept for the people who asked.',
			para('Every essay carries a shadow draft of sentences that lost. This month we keep them: the lede that explained nothing, the metaphor that explained too much.'),
			para('Members see the cutting-room floor because the floor is where the editing actually happens.')
		),
		'role:member'
	),
	gate(
		post(
			'Early look: the release-pinning essay',
			'early-look-release-pinning',
			'Tomas Reyes',
			'2026-11-02',
			['members', 'engineering'],
			'The November feature, a week early for members: how a generation number became the whole consistency story.',
			para('A first reader’s pass, before copy-edit: the argument is here, the examples are not all checked yet.'),
			bullets('What changed since the first draft', 'Where the analogy still creaks', 'The graph we may yet drop')
		),
		'role:member'
	),
	gate(
		post(
			'Signed-in extra: the house style sheet, annotated',
			'signed-in-extra-style-sheet-annotated',
			'Priya Nair',
			'2026-10-31',
			['craft'],
			'Any signed-in member can read this one: the public style-sheet piece with the committee’s margin notes left in.',
			para('Published to everyone who is signed in — no role required, just a session. The annotations are the arguments the final text had to settle.')
		),
		'authenticated'
	),

	/* Fieldnotes, guides */
	guide(
		'Install Fieldnotes',
		'install-fieldnotes',
		'Getting started',
		1,
		'Get the recorder on your device and signed in to your workspace in about five minutes.',
		para('Fieldnotes is a field-recording app: audio, coordinates and text, synced to your workspace when a network exists.'),
		h2('Before you start'),
		bullets('A workspace you can sign in to', 'Ten minutes of connectivity for the first sync'),
		h2('Install it'),
		numbered('Download the build for your platform from the workspace’s Downloads page', 'Open it and sign in with your workspace account', 'Grant microphone and location permissions when asked'),
		callout('tip', 'The first sync is the only one that needs a network. Everything after queues until you are back.')
	),
	guide(
		'Your first recording',
		'your-first-recording',
		'Getting started',
		2,
		'Record a note, tag it, and see it land in the workspace.',
		para('Tap the record button. That is most of it; everything else is optional polish.'),
		numbered('Tap the record button and say what you see', 'Name it and add a tag or two', 'Stop, and watch the sync status flip to synced'),
		callout('note', 'Recordings are insert-only: trimming creates a new revision, never edits the original in place.')
	),
	guide(
		'Connect Fieldnotes to Colophon',
		'connect-to-colophon',
		'Getting started',
		3,
		'Publish field notes into a Colophon collection as drafts, ready for review.',
		para('Fieldnotes can hand its notes to Colophon as drafts over the same HTTP API the Studio uses.'),
		numbered('Create a project API token with draft write access', 'Paste it into Fieldnotes’ integrations page', 'Pick the collection notes land in'),
		para('Notes arrive as drafts. Nothing Fieldnotes sends is ever published without a person’s changeset.')
	),
	guide(
		'Organising recordings',
		'organising-recordings',
		'Guides',
		1,
		'Tags, notebooks and saved searches; pick one system early and stay in it.',
		para('A hundred recordings is a library; ten thousand is a pile. The difference is one habit, chosen early.'),
		bullets('One notebook per project or trip', 'Tags for cross-cutting things: species, weather, gear', 'Saved searches for the queries you repeat'),
		quote(para('The best organisation system is the one you can keep on a wet Tuesday in November.'))
	),
	guide(
		'Sharing a notebook',
		'sharing-a-notebook',
		'Guides',
		2,
		'Share read-only or with-comment access to a whole notebook.',
		para('Sharing is per notebook, never per recording. A colleague sees everything inside it and nothing outside.'),
		numbered('Open the notebook’s share sheet', 'Invite by email; choose read-only or comment', 'Revoke from the same sheet when the work ends'),
		callout('warning', 'A shared notebook shows new recordings to everyone on it. Move sensitive notes out first.')
	),
	guide(
		'Offline capture',
		'offline-capture',
		'Guides',
		3,
		'What Fieldnotes does with no signal: everything, in order, synced later.',
		para('Offline is the default state, not an error. The app queues writes locally and reconciles when a network appears.'),
		para('Conflicts are rare because recordings never update in place, so sync is an ordered merge of inserts.'),
		h2('What to watch'),
		bullets('Battery: recording for hours needs a pack', 'Storage: a full day of audio is about a gigabyte', 'The sync queue tells you exactly what is unsent')
	),
	guide(
		'The notebook format',
		'notebook-format',
		'Reference',
		1,
		'How a notebook is stored: an append-only log of note records and their revisions.',
		para('A notebook is an append-only log. A note’s latest revision is its head; every earlier revision stays readable.'),
		h2('Record kinds'),
		bullets('note: a recording, its transcript and location', 'revision: a named state of a note', 'attachment: media bytes referenced by id'),
		para('The format is documented because exports are plain JSONL, so what you see is literally what is stored.')
	),
	guide(
		'Sync conflicts and how they resolve',
		'sync-conflicts',
		'Reference',
		2,
		'Two devices recording the same notebook can never corrupt each other, because inserts are merged in order.',
		para('Because notes are insert-only, a "conflict" is just two appends. The log orders them by device timestamp, then device id.'),
		para('The single genuinely conflicting case is two edits of one note on two devices: both land as revisions, and the later one wins the head pointer. Nothing is lost.')
	),
	guide(
		'Retention rules',
		'retention-rules',
		'Reference',
		3,
		'How long recordings, transcripts and tombstones live, and who can change it.',
		para('Retention is a workspace policy: recordings default to kept-forever, tombstones to 90 days.'),
		para('A deleted note is tombstoned first (recoverable through support within the window), then removed.'),
		callout('note', 'Export regularly if you answer to a records policy; retention governs the service, not your archive.')
	),
	guide(
		'Automated exports',
		'automated-exports',
		'Advanced',
		1,
		'Nightly JSONL exports to your own bucket, signed and checksummed.',
		para('Workspaces on the field plan get a nightly export: one JSONL per notebook, a manifest, and a SHA-256 manifest signature.'),
		para('The export is additive (a new file per day), so a bad day never overwrites a good one.')
	),
	guide(
		'Running a self-hosted relay',
		'self-hosted-relay',
		'Advanced',
		2,
		'For teams whose field sites can never see the public internet: a store-and-forward relay.',
		para('A relay is a small service that accepts sync traffic over a local link and forwards it when its own uplink exists.'),
		numbered('Run the relay container somewhere the devices can reach', 'Point the app’s relay setting at it', 'The relay signs bundles; the workspace verifies them'),
		callout('warning', 'A relay multiplies your attack surface by exactly one relay. Patch it like a server, because it is one.')
	),

	/* The Meridian Post, stories */
	story(
		'Open protocol lets city sensors outlive their vendors',
		'city-sensors-open-protocol',
		'Technology',
		'2026-11-03',
		'Ada Whitcombe, technology correspondent',
		'A municipal consortium has published the wiring and protocol notes that let a city keep its air-quality sensors alive after the supplier walks away.',
		true,
		para('The consortium of eleven mid-sized cities published its sensor interoperability notes on Monday, the result of two years of refusing proprietary lock-in at procurement time.'),
		para('The specification covers power budgets, mounting, and a modest data format simple enough that a secondary school implemented a reader in a week.'),
		quote(para('A city is a hundred-year customer. The vendor is not.')),
		para('Three manufacturers have already shipped conforming firmware. The real test, engineers say, arrives in year seven, when the first contract expires.')
	),
	story(
		'Harbour towns vote to twin their tide gauges',
		'harbour-towns-twin-tide-gauges',
		'World',
		'2026-11-02',
		'Jonas Ferry, world desk',
		'Forty small harbours will pool readings into one public feed, ending a patchwork of incompatible local gauges.',
		false,
		para('Harbour masters from forty towns voted on Saturday to twin their tide gauges and publish the readings as a single public feed.'),
		para('Each gauge keeps local control; the twin is a shared record sailors can read without visiting forty different websites.'),
		para('The first unified feed is expected before the spring tides.')
	),
	story(
		'The night shift librarians keeping archives awake',
		'night-shift-librarians',
		'Culture',
		'2026-11-01',
		'Imogen Dale',
		'While the city sleeps, a small crew digitises council minutes going back to 1887, and catches what the scanners miss.',
		false,
		para('Between midnight and six, the basement of the central library hums with the sound of page-turning scanners.'),
		para('The crew of five has digitised four decades of council minutes this year. Their margin notes (coffee rings, torn corners, the occasional pressed leaf) go into a parallel index the scanners cannot read.'),
		quote(para('The page is data, but so is the stain. Both say how the record was kept.'))
	),
	story(
		'Amateur survey spots a comet the surveys missed',
		'amateur-survey-spots-comet',
		'Science',
		'2026-10-30',
		'Dr Helena Voss, science editor',
		'A retiree with a shed-mounted telescope flagged an object the automated sky surveys had logged and discarded as noise.',
		false,
		para('The comet, provisionally named for the village where it was spotted, was sitting in public survey data for eleven months.'),
		para('Professional surveys flagged it automatically as a probable artefact. A human noticed its motion was wrong for noise and right for a long-period orbit.'),
		para('Confirmation observations are scheduled through the winter.')
	),
	story(
		'The five-a-side league that tracks its own offsides',
		'five-a-side-tracks-own-offsides',
		'Sport',
		'2026-10-28',
		'Callum Price',
		'A Sunday league built its own camera rig and publishes every disputed decision with the footage.',
		false,
		para('The Meridian Sunday League has published every disputed offside call this season beside the phone-camera footage the call was made from.'),
		para('Disputes are down by half. Players, organisers say, argue differently when the evidence is on the internet by Tuesday.'),
		para('Other leagues have asked for the build notes, which the league publishes under an open licence.')
	),
	story(
		'River ferries return after forty years ashore',
		'river-ferries-return',
		'World',
		'2026-10-27',
		'Jonas Ferry, world desk',
		'A restored ferry service crosses the estuary again, crewed by volunteers and timed by the tide tables it always used.',
		false,
		para('The first crossing in forty years took eleven minutes and carried a brass band, two councillors and a dog.'),
		para('The service runs weekends until the winter refit, then daily for the season.'),
		para('Tickets are sold from a wooden booth that survived the service’s closure in a museum storeroom.')
	),
	story(
		'Repair cafés teach a generation to solder',
		'repair-cafes-teach-soldering',
		'Technology',
		'2026-10-26',
		'Ada Whitcombe, technology correspondent',
		'Monthly repair cafés report waiting lists for the first time, and a waiting list of retired engineers to run them.',
		false,
		para('The kettles are the easy part. It is the laptops, says volunteer coordinator Beth Amara, that draw the crowds.'),
		para('The cafés’ rule is that the owner does the repair; the volunteer holds the light and talks it through.'),
		quote(para('You cannot learn to fix a thing by watching someone else fix it.'))
	),
	story(
		'A foundry casts bells the medieval way',
		'foundry-casts-bells-medieval-way',
		'Culture',
		'2026-10-24',
		'Imogen Dale',
		'Clay, horsehair and a week of patience: the last bell foundry in the region still works as it did in 1400.',
		false,
		para('The mould is loam and horsehair, built up over weeks and destroyed by the casting it exists for.'),
		para('The foundry casts perhaps twelve bells a year, each tuned by shaving the inside with a lathe older than some of its customers.'),
		para('A new treble bell leaves for the cathedral restoration in the spring.')
	),
	story(
		'Urban wetlands quietly return to the flood plain',
		'urban-wetlands-return',
		'Science',
		'2026-10-22',
		'Dr Helena Voss, science editor',
		'Where a car park stood, a reed bed now takes the river’s overflow, and the flood insurance maps are being redrawn.',
		false,
		para('The wetland began as a flood scheme and became, almost by accident, the city’s most visited nature reserve.'),
		para('Reed warblers arrived in year two. The otters arrived when the fish did, which the engineers describe as the review that mattered most.'),
		para('The scheme’s flood models predicted a dry park most winters. The otters were not consulted.')
	),
	story(
		'Marathon runners pace each other to a shared record',
		'marathon-shared-record',
		'Sport',
		'2026-10-20',
		'Callum Price',
		'Four club runners agreed to finish together or not at all. They crossed the line in identical time, eleven seconds under the club record.',
		false,
		para('The pact was made at mile eighteen, when the fastest of the four dropped back rather than run alone.'),
		para('All four were credited with the same gun time. The club record books list them alphabetically.'),
		para('The four say they will defend the record together, or not at all.')
	),

	/* Butter & Thyme, recipes */
	{
		collection: 'recipes',
		content: {
			title: 'Brown butter soda bread',
			slug: 'brown-butter-soda-bread',
			intro: 'Soda bread already forgives a heavy hand. Brown butter makes it taste like you meant it.',
			category: 'Baking',
			difficulty: 'Easy',
			prep_minutes: 15,
			cook_minutes: 40,
			servings: 8,
			ingredients: [
				{ item: 'plain flour', amount: '450 g' },
				{ item: 'bicarbonate of soda', amount: '1 tsp' },
				{ item: 'fine salt', amount: '1 tsp' },
				{ item: 'buttermilk', amount: '400 ml' },
				{ item: 'butter, browned and cooled', amount: '60 g' },
				{ item: 'rolled oats, for the top', amount: '1 tbsp' }
			],
			steps: [
				'Heat the oven to 200°C (180°C fan). Brown the butter in a small pan until it smells nutty, then leave it to cool.',
				'Mix the flour, bicarbonate of soda and salt in a large bowl.',
				'Stir the cooled brown butter into the buttermilk, then pour it into the dry ingredients and bring together with a fork; do not knead.',
				'Tip onto a floured surface, shape into a round about 4 cm high, and slash a deep cross in the top.',
				'Bake for 35–40 minutes until the loaf sounds hollow when tapped underneath. Scatter the oats on top for the last 10 minutes.',
				'Cool on a rack for at least 20 minutes before cutting, since the crumb is still setting.'
			],
			notes: richText(callout('tip', 'A wet hand makes a rounder loaf: dampen your palm before shaping.'))
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Weeknight mushroom ragu',
			slug: 'weeknight-mushroom-ragu',
			intro: 'Deeply savoury, done in half an hour, and better the next day if any survives.',
			category: 'Mains',
			difficulty: 'Easy',
			prep_minutes: 10,
			cook_minutes: 25,
			servings: 4,
			ingredients: [
				{ item: 'mixed mushrooms, sliced', amount: '600 g' },
				{ item: 'olive oil', amount: '3 tbsp' },
				{ item: 'onion, finely chopped', amount: '1' },
				{ item: 'garlic cloves, sliced', amount: '3' },
				{ item: 'tomato purée', amount: '2 tbsp' },
				{ item: 'red wine', amount: '150 ml' },
				{ item: 'chopped tomatoes', amount: '400 g tin' },
				{ item: 'dried rigatoni', amount: '400 g' },
				{ item: 'butter', amount: '30 g' },
				{ item: 'parmesan, grated', amount: 'to serve' }
			],
			steps: [
				'Sear the mushrooms in batches in hot olive oil until deeply browned; crowd them and they will stew.',
				'Soften the onion in the same pan for 8 minutes, add the garlic for 1 minute more.',
				'Stir in the tomato purée, cook for a minute, then add the wine and let it bubble away by half.',
				'Add the tomatoes and a splash of water; simmer 12 minutes until thick. Season well.',
				'Cook the pasta a minute shy of the packet time, then finish it in the sauce with the butter and a ladle of pasta water.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Roast tomato and red pepper soup',
			slug: 'roast-tomato-red-pepper-soup',
			intro: 'Roasting does the work here; the blender just takes the credit.',
			category: 'Soups and salads',
			difficulty: 'Easy',
			prep_minutes: 10,
			cook_minutes: 45,
			servings: 6,
			ingredients: [
				{ item: 'tomatoes, halved', amount: '1 kg' },
				{ item: 'red peppers, deseeded and quartered', amount: '3' },
				{ item: 'onion, quartered', amount: '1' },
				{ item: 'garlic bulb, top sliced off', amount: '1' },
				{ item: 'olive oil', amount: '4 tbsp' },
				{ item: 'vegetable stock', amount: '750 ml' },
				{ item: 'smoked paprika', amount: '1 tsp' },
				{ item: 'single cream', amount: 'to serve (optional)' }
			],
			steps: [
				'Heat the oven to 200°C (180°C fan). Roast the tomatoes, peppers, onion and garlic in olive oil for 40 minutes, until the edges catch.',
				'Squeeze the garlic from its skin into a blender with everything else from the tray, plus the stock and paprika.',
				'Blend until completely smooth, then warm through in a pan. Taste first: roasting concentrates everything, so season carefully.',
				'Serve with a swirl of cream and very good bread.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Cardamom rice pudding',
			slug: 'cardamom-rice-pudding',
			intro: 'The nursery classic, perfumed rather than plain. Serve it barely warm, not hot.',
			category: 'Puddings',
			difficulty: 'Medium',
			prep_minutes: 5,
			cook_minutes: 60,
			servings: 6,
			ingredients: [
				{ item: 'short-grain pudding rice', amount: '150 g' },
				{ item: 'whole milk', amount: '1 litre' },
				{ item: 'caster sugar', amount: '60 g' },
				{ item: 'green cardamom pods, lightly crushed', amount: '8' },
				{ item: 'bay leaf', amount: '1' },
				{ item: 'unsalted butter', amount: '20 g' },
				{ item: 'nutmeg, grated', amount: 'for the top' }
			],
			steps: [
				'Heat the oven to 150°C (130°C fan). Butter a shallow ovenproof dish.',
				'Stir the rice, milk, sugar, cardamom and bay together in the dish and dot with butter.',
				'Bake for 1 hour 30 minutes, stirring once halfway, until the rice is tender and the top is golden.',
				'Rest for 15 minutes, fish out the pods and bay, grate over nutmeg and serve barely warm.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Slow lamb shoulder with harissa',
			slug: 'slow-lamb-shoulder-harissa',
			intro: 'Five minutes of work, five hours of oven. The kind of cooking that makes a flat smell like a restaurant.',
			category: 'Mains',
			difficulty: 'A proper project',
			prep_minutes: 15,
			cook_minutes: 300,
			servings: 6,
			ingredients: [
				{ item: 'lamb shoulder, bone in', amount: '2 kg' },
				{ item: 'rose harissa', amount: '4 tbsp' },
				{ item: 'garlic cloves, crushed', amount: '4' },
				{ item: 'preserved lemon, finely chopped', amount: '1' },
				{ item: 'olive oil', amount: '2 tbsp' },
				{ item: 'red onions, wedged', amount: '3' },
				{ item: 'chickpeas, drained', amount: '400 g tin' },
				{ item: 'Greek yoghurt and coriander', amount: 'to serve' }
			],
			steps: [
				'Heat the oven to 160°C (140°C fan). Mix the harissa, garlic, preserved lemon and oil into a paste.',
				'Score the lamb deeply and rub the paste all over, into the cuts. Season with salt.',
				'Put the onions in a roasting tin, sit the lamb on top, add a mug of water and cover tightly with foil.',
				'Roast for 4½ hours, then uncover, add the chickpeas and roast 30 minutes more until the lamb pulls apart with a fork.',
				'Rest 20 minutes, then pull the meat into the pan juices. Serve with yoghurt, coriander and flatbreads.'
			],
			notes: richText(callout('note', 'The shoulder shrugs off an extra hour if you are running late (keep it covered).'))
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Lemon drizzle loaf',
			slug: 'lemon-drizzle-loaf',
			intro: 'The sharpest cake in the tin. The crunch on top is a deliberate act of under-dissolving the sugar.',
			category: 'Baking',
			difficulty: 'Easy',
			prep_minutes: 15,
			cook_minutes: 50,
			servings: 10,
			ingredients: [
				{ item: 'unsalted butter, soft', amount: '175 g' },
				{ item: 'caster sugar', amount: '175 g' },
				{ item: 'eggs', amount: '3' },
				{ item: 'self-raising flour', amount: '175 g' },
				{ item: 'lemons, zested and juiced', amount: '2' },
				{ item: 'granulated sugar, for the drizzle', amount: '85 g' }
			],
			steps: [
				'Heat the oven to 180°C (160°C fan). Line a 900 g loaf tin.',
				'Beat the butter and caster sugar until pale, then beat in the eggs one at a time.',
				'Fold in the flour and lemon zest, then the juice of one lemon. Scrape into the tin.',
				'Bake 45–50 minutes until a skewer comes out clean.',
				'Mix the remaining lemon juice with the granulated sugar without dissolving it and spoon over the hot loaf. Cool in the tin.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Grilled peach and burrata salad',
			slug: 'grilled-peach-burrata-salad',
			intro: 'August on a plate: charred fruit, cold cheese, hot honey. Ready before the barbecue warms up.',
			category: 'Soups and salads',
			difficulty: 'Easy',
			prep_minutes: 10,
			cook_minutes: 8,
			servings: 4,
			ingredients: [
				{ item: 'ripe peaches, halved and stoned', amount: '4' },
				{ item: 'burrata', amount: '2 balls' },
				{ item: 'olive oil', amount: '2 tbsp' },
				{ item: 'runny honey', amount: '2 tbsp' },
				{ item: 'chilli flakes', amount: 'a pinch' },
				{ item: 'rocket', amount: '80 g' },
				{ item: 'toasted hazelnuts, chopped', amount: '30 g' },
				{ item: 'lemon juice', amount: 'of half a lemon' }
			],
			steps: [
				'Heat a griddle until smoking. Brush the peach halves with oil and grill cut-side down for 3–4 minutes until striped.',
				'Dress the rocket with lemon juice and a little oil, then pile onto a platter.',
				'Tear over the burrata, arrange the peaches, and drizzle with honey warmed with the chilli flakes.',
				'Scatter the hazelnuts, season with flaky salt and serve immediately.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Chocolate olive oil cake',
			slug: 'chocolate-olive-oil-cake',
			intro: 'Dairy-free by accident and fudge-dark by design. Nobody has ever guessed the oil.',
			category: 'Baking',
			difficulty: 'Medium',
			prep_minutes: 20,
			cook_minutes: 40,
			servings: 10,
			ingredients: [
				{ item: 'good cocoa powder', amount: '60 g' },
				{ item: 'boiling water', amount: '150 ml' },
				{ item: 'olive oil, fruity not peppery', amount: '180 ml' },
				{ item: 'caster sugar', amount: '200 g' },
				{ item: 'eggs', amount: '3' },
				{ item: 'ground almonds', amount: '150 g' },
				{ item: 'bicarbonate of soda', amount: '½ tsp' },
				{ item: 'flaky salt', amount: 'a pinch' }
			],
			steps: [
				'Heat the oven to 170°C (150°C fan). Line a 20 cm round tin.',
				'Whisk the cocoa and boiling water to a paste, then whisk in the oil and leave to cool a little.',
				'Beat the eggs and sugar until thick and pale, then whisk in the cocoa mixture.',
				'Fold in the almonds, bicarb and salt. Pour into the tin; the batter is very loose, which is correct.',
				'Bake 40–45 minutes until the top is firm but the centre still has a wobble. Cool completely before slicing.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Smoked haddock chowder',
			slug: 'smoked-haddock-chowder',
			intro: 'A chowder should be a meal, not a soup. This one is: potatoes, sweetcorn and proper smoked fish.',
			category: 'Soups and salads',
			difficulty: 'Medium',
			prep_minutes: 15,
			cook_minutes: 30,
			servings: 4,
			ingredients: [
				{ item: 'undyed smoked haddock', amount: '400 g' },
				{ item: 'whole milk', amount: '500 ml' },
				{ item: 'butter', amount: '40 g' },
				{ item: 'leek, sliced', amount: '1' },
				{ item: 'floury potatoes, diced', amount: '400 g' },
				{ item: 'sweetcorn', amount: '200 g' },
				{ item: 'bay leaves', amount: '2' },
				{ item: 'double cream', amount: '100 ml' },
				{ item: 'chives, chopped', amount: 'to serve' }
			],
			steps: [
				'Poach the haddock in the milk with the bay for 5 minutes, then lift it out, flake it and keep the milk.',
				'Soften the leek in butter for 8 minutes without colour.',
				'Add the potatoes, the poaching milk and just enough water to cover. Simmer 12 minutes until the potatoes give.',
				'Add the sweetcorn and cream, simmer 3 minutes, then return the flaked fish and warm through gently, never boiling.',
				'Season with white pepper, scatter chives, and serve with the brownest bread you have.'
			]
		}
	},
	{
		collection: 'recipes',
		content: {
			title: 'Fig and almond tart',
			slug: 'fig-and-almond-tart',
			intro: 'Frangipane is the most forgiving of the fancy fillings, and figs forgive even less than the pastry.',
			category: 'Puddings',
			difficulty: 'A proper project',
			prep_minutes: 40,
			cook_minutes: 45,
			servings: 8,
			ingredients: [
				{ item: 'sweet shortcrust pastry', amount: '300 g' },
				{ item: 'unsalted butter, soft', amount: '100 g' },
				{ item: 'caster sugar', amount: '100 g' },
				{ item: 'eggs', amount: '2' },
				{ item: 'ground almonds', amount: '125 g' },
				{ item: 'plain flour', amount: '1 tbsp' },
				{ item: 'ripe figs, quartered', amount: '6' },
				{ item: 'flaked almonds', amount: '20 g' },
				{ item: 'apricot jam, warmed', amount: '2 tbsp' }
			],
			steps: [
				'Line a 23 cm fluted tart tin with the pastry, chill 20 minutes, then blind-bake at 190°C (170°C fan) for 15 minutes.',
				'Beat the butter and sugar until pale, beat in the eggs one at a time, then fold in the ground almonds and flour.',
				'Spread the frangipane in the case and press the figs in, cut side up. Scatter the flaked almonds.',
				'Bake 30–35 minutes until the frangipane is golden and springs back.',
				'Brush with the warmed jam while hot. Serve at room temperature, with cream if the day calls for it.'
			]
		}
	}
];

/* -------------------------------------------------------------- the flow */

/** Uploads `bytes` as `file`, then waits for the verify job to mark it usable (or reject it). */
async function uploadPhoto(base: string, file: string, bytes: Buffer): Promise<string> {
	const response = await fetch(`${origin}${base}/assets`, {
		method: 'POST',
		headers: {
			cookie: cookieHeader(),
			origin,
			'x-csrf-token': cookies.get('colophon_csrf') ?? '',
			'content-type': 'image/png',
			'x-colophon-file-name': file,
			'idempotency-key': key()
		},
		body: new Uint8Array(bytes)
	});
	const text = await response.text();
	if (response.status !== 202) throw new Error(`upload ${file}: HTTP ${response.status} ${text}`);
	const asset = (JSON.parse(text) as { data: { id: string } }).data.id;
	for (let wait = 0; wait < 120; wait += 1) {
		const got = await api<{ state: string; rejected_reason?: string }>('GET', `${base}/assets/${asset}`);
		if (got.state === 'usable') return asset;
		if (got.state === 'rejected') throw new Error(`${file} rejected: ${got.rejected_reason ?? 'no reason given'}`);
		await new Promise((done) => setTimeout(done, 1000));
	}
	throw new Error(`${file} was still quarantined after two minutes`);
}

const PHOTO_CONTENT: Array<[string, string, string, string, string, string]> = [
	// file, title, caption, location, taken_on, series
	['overpass-dusk.png', 'Overpass at dusk', 'The last of the light under the eastbound carriageway.', 'Leeds', '2026-06-14', 'Urban'],
	['night-bus-depot.png', 'Night bus depot', 'Sodium light and parked buses, half past two.', 'Glasgow', '2026-03-02', 'Urban'],
	['fellside-rain.png', 'Fellside after rain', 'The path shines more than the sky does.', 'Lake District', '2026-04-19', 'Landscape'],
	['salt-marsh.png', 'Salt marsh morning', 'Channels filling on the turn of the tide.', 'North Norfolk', '2026-05-30', 'Landscape'],
	['chess-tables.png', 'The chess tables', 'The regulars play through the drizzle. Nobody looks up.', 'Hyde Park', '2026-07-11', 'People'],
	['market-porters.png', 'Market porters', 'The last hour of the wholesale market.', 'New Spitalfields', '2026-08-22', 'People'],
	['darkroom-tools.png', 'Darkroom tools', 'Safelight, tongs, and forty years of habit.', 'The darkroom', '2026-01-17', 'Still life'],
	['winter-apples.png', 'Winter apples', 'Stored fruit, shot on the pantry window ledge.', 'Herefordshire', '2026-01-28', 'Still life'],
	['tidal-flats.png', 'Tidal flats, low water', 'A mile of wet sand and one dog walker.', 'Morecambe Bay', '2026-09-05', 'Landscape'],
	['glasshouse.png', 'Glasshouse, first frost', 'The tomatoes are done; the glass steams anyway.', 'The allotment', '2026-10-18', 'Still life']
];

async function main(): Promise<void> {
	await signIn();
	const slug = `demo-sites-${Date.now().toString(36)}`;
	const project = (await api<{ id: string }>('POST', '/api/v1/projects', { slug, name: 'Demo sites' })).id;
	const base = `/api/v1/projects/${project}/environments/production`;
	console.log(`project ${project} (${slug})`);

	for (const type of Object.values(COLLECTIONS)) {
		await api('POST', `${base}/content-types`, type, { 'idempotency-key': key() });
	}
	console.log(`content types: ${Object.keys(COLLECTIONS).join(', ')}`);

	// Audience gates first: `role:<name>` visibility must name a real audience role (D-137).
	// `member` is open — visitors take it at signup — and the seeded member account holds it;
	// `vip` is closed, so the seed can show that `role:vip` entries exist even while no visitor
	// can reach them.
	for (const [name, selfAssign] of [['member', true], ['vip', false]] as const) {
		await api('POST', `/api/v1/projects/${project}/audience/roles`, { name, self_assign: selfAssign });
	}
	console.log('audience roles: member (open), vip (closed)');

	const member = await api<{ id: string }>(
		'POST',
		`/api/v1/projects/${project}/audience/users`,
		{ email: 'member@draft.test', password: 'member-of-the-draft', display_name: 'Demo member', role: 'member' }
	);
	console.log(`audience user ${member.id} (member@draft.test)`);

	// Photographs first: entries need usable asset ids, and verification is a worker's job.
	const assets = new Map<string, string>();
	for (const photo of PHOTOS) {
		const bytes = png(W, H, photo.pixel);
		assets.set(photo.file, await uploadPhoto(base, photo.file, bytes));
	}
	console.log(`assets usable: ${assets.size}`);

	const entries: string[] = [];
	for (const seed of SEEDS) {
		const gated = seed.visibility === undefined ? '' : `?visibility=${encodeURIComponent(seed.visibility)}`;
		const created = await api<{ id: string }>(
			'POST',
			`${base}/collections/${seed.collection}/entries${gated}`,
			seed.content,
			{ 'idempotency-key': key() }
		);
		entries.push(created.id);
	}
	for (const [file, title, caption, location, taken_on, series] of PHOTO_CONTENT) {
		const created = await api<{ id: string }>(
			'POST',
			`${base}/collections/photos/entries`,
			{ title, slug: title.toLowerCase().replaceAll(/[^a-z0-9]+/g, '-').replaceAll(/^-|-$/g, ''), photo: assets.get(file), caption, location, taken_on, series },
			{ 'idempotency-key': key() }
		);
		entries.push(created.id);
	}
	console.log(`entries: ${entries.length}`);

	const changeset = await api<{ id: string; hash: string }>(
		'POST',
		`${base}/changesets`,
		{ title: 'Launch all five sites', entries },
		{ 'idempotency-key': key() }
	);
	await api('POST', `${base}/changesets/${changeset.id}/submit`, undefined, { 'if-match': `"${changeset.hash}"` });
	await api('POST', `${base}/changesets/${changeset.id}/reviews`, { decision: 'approve', comment: 'Ready for the five sites.' }, { 'if-match': `"${changeset.hash}"` });
	const release = await api<{ generation: number }>(
		'POST',
		`${base}/changesets/${changeset.id}/publish`,
		{ base_generation: 0 },
		{ 'if-match': `"${changeset.hash}"`, 'idempotency-key': key() }
	);
	console.log(`release ${release.generation} published`);

	// Anonymous reads prove the release is served before the record is written.
	for (const collection of Object.keys(COLLECTIONS)) {
		const response = await fetch(`${origin}/content/v1/${project}/production/${collection}?limit=100`);
		if (!response.ok) throw new Error(`delivery ${collection}: HTTP ${response.status}`);
		const page = (await response.json()) as { data?: Array<{ title: string }> };
		console.log(`  ${collection}: ${page.data?.length ?? 0} published (generation ${response.headers.get('colophon-generation')})`);
	}

	writeFileSync(
		RECORD,
		`${JSON.stringify(
			{
				api: origin,
				project,
				environment: 'production',
				generation: release.generation,
				collections: Object.keys(COLLECTIONS),
				seeded_at: new Date().toISOString()
			},
			null,
			'\t'
		)}\n`
	);
	console.log(`wrote ${RECORD}`);
	console.log('the draft members: member@draft.test / member-of-the-draft (role member)');
	console.log(project);
}

await main();
