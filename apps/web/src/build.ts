// A pinned build (docs/modules/15-web-blog.md, "Pinned build and promotion"; D-052): every read
// of the public API names one release, so a build can't mix two even if a new one is published
// while it runs. Without a release asked for, the first read's Colophon-Generation decides it and
// every later read pins to that. The output goes into a new folder of its own; promote.ts decides
// whether it goes live.

import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { type Article, STYLE, article, articlePath, colophon, home, notFound, type Release } from './pages.ts';

export interface BuildOptions {
	/** Colophon's origin, such as http://127.0.0.1:58000. */
	api: string;
	project: string;
	environment: string;
	/** The release to build; the latest when left out. */
	generation?: number;
	/** The site's root folder: builds go in its `builds/`. */
	root: string;
	/** For tests. */
	fetch?: typeof fetch;
	now?: Date;
}

export interface Built {
	generation: number;
	/** The build's folder. */
	dir: string;
	/** Every page written, by path. */
	pages: string[];
}

export class BuildFailed extends Error {
	override name = 'BuildFailed';
}

/** The most pages of articles a build reads: 100 per page, so 10,000 articles. */
const MAX_PAGES = 100;

interface Page {
	data: Article[];
	next_cursor: string | null;
}

/** Reads every article of the release: the release it is, and the articles, newest first. */
async function articles(options: BuildOptions): Promise<{ generation: number; items: Article[] }> {
	const get = options.fetch ?? fetch;
	const base = `${options.api}/content/v1/${encodeURIComponent(options.project)}/${encodeURIComponent(options.environment)}/articles`;
	let generation = options.generation;
	let cursor: string | null = null;
	const items: Article[] = [];
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const query = new URLSearchParams({ limit: '100' });
		if (cursor !== null) query.set('cursor', cursor);
		else if (generation !== undefined) query.set('generation', String(generation));
		const response = await get(`${base}?${query}`, { headers: { accept: 'application/json' } });
		if (!response.ok) throw new BuildFailed(`reading articles: HTTP ${response.status}`);
		const served = Number(response.headers.get('colophon-generation'));
		if (!Number.isSafeInteger(served) || served < 0) throw new BuildFailed('the answer names no release');
		if (generation === undefined) generation = served;
		if (served !== generation) throw new BuildFailed(`asked for release ${generation}, got ${served}`);
		const body = (await response.json()) as Page;
		items.push(...body.data);
		cursor = body.next_cursor;
		if (cursor === null) return { generation, items };
	}
	throw new BuildFailed(`more than ${MAX_PAGES * 100} articles`);
}

/** Builds the release into a new folder under `root/builds`. Nothing live changes. */
export async function build(options: BuildOptions): Promise<Built> {
	const { generation, items } = await articles(options);
	const release: Release = { generation, builtAt: options.now ?? new Date() };
	const dir = join(options.root, 'builds', `${generation}-${randomUUID()}`);
	const files = new Map<string, string>([
		['/index.html', home(release, items)],
		['/colophon/index.html', colophon(release, items)],
		['/404.html', notFound(release)],
		['/style.css', STYLE],
		['/generation.txt', `${generation}\n`]
	]);
	for (const item of items) {
		files.set(`${articlePath(item)}index.html`, article(release, item));
	}
	for (const [path, text] of files) {
		const target = join(dir, path);
		await mkdir(join(target, '..'), { recursive: true });
		await writeFile(target, text, { flag: 'wx' });
	}
	return { generation, dir, pages: [...files.keys()].filter((path) => path.endsWith('.html')) };
}
