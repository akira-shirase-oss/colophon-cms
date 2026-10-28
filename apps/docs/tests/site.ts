// What the tests know about the built site: where it is, its base path, and every page in it.

import { execFileSync } from 'node:child_process';
import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

export const BUILD = join(import.meta.dirname, '..', 'build');
export const REPO = join(import.meta.dirname, '..', '..', '..');
/** The base path the docs are built for (lib/markdown/addresses.ts). */
export const BASE = process.env['COLOPHON_DOCS_BASE'] ?? '/docs';

/** A page's address as the browser asks for it: `/concepts/` is `/docs/concepts/`. */
export function docs(address: string): string {
	return `${BASE}${address}`;
}

/** Every prerendered page's address inside the docs, `/` first: each `index.html` under build/. */
export function builtPages(): string[] {
	if (!existsSync(join(BUILD, 'index.html'))) {
		throw new Error('no build: run `npm run build -w @colophon/docs` before the tests');
	}
	const pages: string[] = [];
	const walk = (dir: string) => {
		for (const name of readdirSync(dir)) {
			const path = join(dir, name);
			if (statSync(path).isDirectory()) {
				if (name !== '_app' && name !== 'pagefind') walk(path);
			} else if (name === 'index.html') {
				const folder = relative(BUILD, dir).split(sep).join('/');
				pages.push(folder === '' ? '/' : `/${folder}/`);
			}
		}
	};
	walk(BUILD);
	return pages.sort((a, b) => (a === '/' ? -1 : b === '/' ? 1 : a.localeCompare(b)));
}

/** A built page's HTML, by its address inside the docs. */
export function builtHtml(address: string): string {
	return readFileSync(join(BUILD, ...address.split('/').filter(Boolean), 'index.html'), 'utf8');
}

const ENTITIES: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', '#39': "'", '#x27': "'", '#123': '{', '#125': '}' };

/** Text of an HTML fragment: tags dropped, entities decoded. */
export function textOf(html: string): string {
	return html
		.replace(/<[^>]+>/g, '')
		.replace(/&(amp|lt|gt|quot|#39|#x27|#123|#125);/g, (_, name: string) => ENTITIES[name] ?? '');
}

/**
 * The docs describe the code they ship with, so the tests read it from the checked-out tree. To
 * check them against another commit instead (a tag, or a branch you have fetched), set
 * COLOPHON_DOCS_CODE_REF and the files are read from it with `git show`.
 */
export const CODE_REF = process.env['COLOPHON_DOCS_CODE_REF'] || undefined;

/** A source file the docs describe: from the checked-out tree, or from CODE_REF when it is set. */
export function codeFile(path: string): string {
	if (CODE_REF === undefined) {
		try {
			return readFileSync(join(REPO, path), 'utf8');
		} catch (error) {
			throw new Error(`could not read ${path} in the checked-out tree (${String(error)})`);
		}
	}
	try {
		return execFileSync('git', ['show', `${CODE_REF}:${path}`], { cwd: REPO, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
	} catch (error) {
		const detail = error instanceof Error && 'stderr' in error ? String((error as { stderr: unknown }).stderr).trim() : String(error);
		throw new Error(`could not read ${path} at COLOPHON_DOCS_CODE_REF=${CODE_REF} (${detail}); fetch that ref, or unset the variable to read the checked-out tree`);
	}
}

/** The files directly in a source directory that end with `extension`, from the repository's root, sorted. */
export function codeFiles(dir: string, extension: string): string[] {
	const names =
		CODE_REF === undefined
			? readdirSync(join(REPO, dir))
			: execFileSync('git', ['ls-tree', '--name-only', `${CODE_REF}:${dir}`], { cwd: REPO, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] }).split('\n');
	return names
		.filter((name) => name.endsWith(extension))
		.sort()
		.map((name) => `${dir}/${name}`);
}
