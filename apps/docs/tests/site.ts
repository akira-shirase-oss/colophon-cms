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

/** The newest code, which the docs describe: the custom-roles branch contains main, 3.6 and R.1. */
export const NEWEST = process.env['COLOPHON_DOCS_NEWEST_REF'] ?? 'origin/r/r.2-custom-roles';

/**
 * A file as it is on a branch, read with `git show`, so the docs are checked against code that
 * isn't merged yet. Fails with a message that says what to do when the branch isn't there.
 */
export function atRef(path: string, ref = NEWEST): string {
	try {
		return execFileSync('git', ['show', `${ref}:${path}`], { cwd: REPO, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, stdio: ['ignore', 'pipe', 'pipe'] });
	} catch (error) {
		const detail = error instanceof Error && 'stderr' in error ? String((error as { stderr: unknown }).stderr).trim() : String(error);
		throw new Error(
			`could not read ${path} at ${ref} (${detail}). The docs describe the newest branch; fetch it ("git fetch origin") or set COLOPHON_DOCS_NEWEST_REF to the ref that has it.`
		);
	}
}
