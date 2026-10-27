// The repository this site is built from. Pages that show the project's own files (the design
// documents, the load results) read them here at build time, so there is one copy of each: the
// file itself.

import { existsSync } from 'node:fs';
import path from 'node:path';

function findRoot(): string {
	let dir = process.cwd();
	for (;;) {
		if (existsSync(path.join(dir, 'docs', 'END_GOAL.md')) && existsSync(path.join(dir, 'Cargo.toml'))) return dir;
		const parent = path.dirname(dir);
		if (parent === dir) throw new Error(`no Colophon repository above ${process.cwd()}`);
		dir = parent;
	}
}

/** The repository's root directory. */
export const ROOT = findRoot();

/** A path inside the repository, from its root. */
export function inRepo(...parts: string[]): string {
	return path.join(ROOT, ...parts);
}
