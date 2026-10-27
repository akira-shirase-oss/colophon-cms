// What the tests know about the built site: where it is, and every page in it.

import { existsSync, readdirSync, readFileSync, statSync } from 'node:fs';
import { join, relative, sep } from 'node:path';

export const BUILD = join(import.meta.dirname, '..', 'build');
export const REPO = join(import.meta.dirname, '..', '..', '..');

/** Every prerendered page's address, `/` first: each `index.html` under build/. */
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

/** A built page's HTML, by address. */
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
