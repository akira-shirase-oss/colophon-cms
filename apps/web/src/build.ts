// A pinned build (docs/modules/15-web-blog.md, "Pinned build and promotion"; D-052): every read
// of the public API names one release, so a build can't mix two even if a new one is published
// while it runs. Without a release asked for, the first read's Colophon-Generation decides it and
// every later read pins to that. The output goes into a new folder of its own; promote.ts decides
// whether it goes live.

import { randomUUID } from 'node:crypto';
import { mkdir, writeFile } from 'node:fs/promises';
import { join } from 'node:path';

import { type Article, STYLE, article, articlePath, colophon, home, notFound, type Release } from './pages.ts';
import { type Entry, siteFiles } from './site.ts';

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

/** The most pages of one collection a build reads: 100 per page, so 10,000 entries. */
const MAX_PAGES = 100;

interface Page<T> {
	data: T[];
	next_cursor: string | null;
}

/**
 * Reads every entry of one collection at the release: the first page asked with no cursor fixes
 * the release (the caller's, or the served one), and every later page must agree — a build can't
 * mix two releases even if a new one is published while it runs.
 */
async function collection<T>(options: BuildOptions, name: string, generation: number | undefined): Promise<{ generation: number; items: T[] }> {
	const get = options.fetch ?? fetch;
	const base = `${options.api}/content/v1/${encodeURIComponent(options.project)}/${encodeURIComponent(options.environment)}/${encodeURIComponent(name)}`;
	let cursor: string | null = null;
	const items: T[] = [];
	for (let page = 0; page < MAX_PAGES; page += 1) {
		const query = new URLSearchParams({ limit: '100' });
		if (cursor !== null) query.set('cursor', cursor);
		else if (generation !== undefined) query.set('generation', String(generation));
		const response = await get(`${base}?${query}`, { headers: { accept: 'application/json' } });
		if (!response.ok) throw new BuildFailed(`reading ${name}: HTTP ${response.status}`);
		const served = Number(response.headers.get('colophon-generation'));
		if (!Number.isSafeInteger(served) || served < 0) throw new BuildFailed('the answer names no release');
		if (generation === undefined) generation = served;
		if (served !== generation) throw new BuildFailed(`asked for release ${generation}, got ${served}`);
		const body = (await response.json()) as Page<T>;
		items.push(...body.data);
		cursor = body.next_cursor;
		if (cursor === null) return { generation, items };
	}
	throw new BuildFailed(`more than ${MAX_PAGES * 100} entries of ${name}`);
}

/** Builds the release into a new folder under `root/builds`. Nothing live changes. */
export async function build(options: BuildOptions): Promise<Built> {
	// The site collections first (W.3): a project with the website starter gets its pages, menu
	// and stylesheet; a project without publishes nothing under `pages`, and the build is the
	// blog alone, exactly as before. Every read pins to the same release.
	const site = await collection<Entry>(options, 'pages', options.generation);
	const articlesRead = await collection<Article>(options, 'articles', site.generation);
	const navigation = await collection<Entry>(options, 'navigation', articlesRead.generation);
	const theme = await collection<Entry>(options, 'theme', navigation.generation);
	const generation = theme.generation;
	const items = articlesRead.items;

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
		const path = `${articlePath(item)}index.html`;
		// Slugs are unique among published entries, but a slug may spell another entry's id: two
		// articles at one address would hide one of them, so the build fails and nothing goes live.
		if (files.has(path)) throw new BuildFailed(`two articles would be served at ${path}`);
		files.set(path, article(release, item));
	}
	// The site's files go over the blog's base: its home replaces the article list at `/` and its
	// 404 replaces the plain one, but a page slug that collides with any other fixed path —
	// /colophon/, an article's — fails the whole build, like two articles at one address.
	let siteMap: Map<string, string>;
	try {
		siteMap = siteFiles(release, site.items, navigation.items[0], theme.items[0], {
			assetUrl: (id) => `/media/v1/${options.project}/${options.environment}/${encodeURIComponent(id)}`
		});
	} catch (error) {
		throw new BuildFailed(error instanceof Error ? error.message : String(error));
	}
	for (const [path, text] of siteMap) {
		if (files.has(path) && path !== '/index.html' && path !== '/404.html') throw new BuildFailed(`a site page would be served at ${path}`);
		files.set(path, text);
	}
	for (const [path, text] of files) {
		const target = join(dir, path);
		await mkdir(join(target, '..'), { recursive: true });
		await writeFile(target, text, { flag: 'wx' });
	}
	return { generation, dir, pages: [...files.keys()].filter((path) => path.endsWith('.html')) };
}
