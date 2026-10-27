// Copies the Colophon build log's posts from the maintainer's Zola site into this site, byte for
// byte: the author's text is never changed here; the site renders it at build time
// (src/lib/server/blog.ts) and names each original as canonical.
//
//   node scripts/import-blog.ts [source directory]
//
// The source defaults to $COLOPHON_BUILD_LOG_DIR, then to minimal-portfolio/content/colophon next
// to the projects folder on the maintainer's machine. Posts that disappeared from the source are
// removed here too, so the two always hold the same set; the section page (_index.md) is not a
// post and is skipped. Every post must have front matter with a title and a date.

import { copyFileSync, readFileSync, readdirSync, rmSync } from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { parse } from 'smol-toml';
import { SECTION_INDEX, canonicalOf, slugOf, splitFrontMatter } from '../src/lib/build-log.ts';

const here = path.dirname(fileURLToPath(import.meta.url));
const target = path.resolve(here, '../src/content/blog');
const source = path.resolve(
	process.argv[2] ??
		process.env['COLOPHON_BUILD_LOG_DIR'] ??
		path.resolve(here, '../../../../../minimal-portfolio/content/colophon')
);

const posts = readdirSync(source)
	.filter((name) => name.endsWith('.md') && name !== SECTION_INDEX)
	.sort();
if (posts.length === 0) throw new Error(`no posts in ${source}`);

const slugs = new Set<string>();
for (const name of posts) {
	const text = readFileSync(path.join(source, name), 'utf8');
	const meta = parse(splitFrontMatter(text).frontMatter);
	if (typeof meta['title'] !== 'string' || !(meta['date'] instanceof Date)) {
		throw new Error(`${name}: front matter needs a title and a date`);
	}
	const slug = slugOf(name, meta['slug']);
	if (slugs.has(slug)) throw new Error(`${name}: slug ${slug} is used twice`);
	slugs.add(slug);
	copyFileSync(path.join(source, name), path.join(target, name));
	console.log(`${name} -> /blog/${slug}/ (canonical ${canonicalOf(slug)})`);
}

for (const name of readdirSync(target)) {
	if (name.endsWith('.md') && !posts.includes(name)) {
		rmSync(path.join(target, name));
		console.log(`${name}: no longer in the source, removed`);
	}
}
console.log(`${posts.length} posts from ${source}`);
