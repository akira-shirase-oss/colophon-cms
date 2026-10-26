// Verify, then promote atomically (docs/modules/15-web-blog.md, invariants B1–B3; D-052). The live
// site is `root/current`, a symbolic link to one build's folder. Promoting checks the build (every
// page carries its release's marker, and nothing else's), then swaps the link with one rename,
// which replaces it atomically: a reader gets the old site or the new one, never half of each.
// Promotions are serialized by a lock folder, and a build of an older release than the live one is
// refused (X08: a slow old build finishing late must not undo a newer release).

import { mkdir, readFile, readdir, readlink, rename, rm, symlink } from 'node:fs/promises';
import { join, relative } from 'node:path';

import type { Built } from './build.ts';
import { marker } from './pages.ts';

export class VerifyFailed extends Error {
	override name = 'VerifyFailed';
}

export type Promoted = { promoted: true; generation: number } | { promoted: false; reason: 'superseded'; live: number };

/** How long promote waits for another promotion's lock before giving up. */
const LOCK_WAIT_MS = 10_000;

async function htmlFiles(dir: string): Promise<string[]> {
	const found: string[] = [];
	for (const entry of await readdir(dir, { withFileTypes: true })) {
		const path = join(dir, entry.name);
		if (entry.isDirectory()) found.push(...(await htmlFiles(path)));
		else if (entry.name.endsWith('.html')) found.push(path);
	}
	return found;
}

/** Checks a build before it can go live: its release file, a home page, and the marker on every
 *  page, naming this release and no other. */
export async function verify(built: Built): Promise<void> {
	const recorded = (await readFile(join(built.dir, 'generation.txt'), 'utf8')).trim();
	if (recorded !== String(built.generation)) throw new VerifyFailed(`generation.txt says ${recorded}, not ${built.generation}`);
	const pages = await htmlFiles(built.dir);
	if (!pages.some((page) => relative(built.dir, page) === 'index.html')) throw new VerifyFailed('no home page');
	const wanted = marker(built.generation);
	for (const page of pages) {
		const html = await readFile(page, 'utf8');
		const markers = html.match(/<meta name="colophon-generation" content="[^"]*">/g) ?? [];
		if (markers.length !== 1 || markers[0] !== wanted) {
			throw new VerifyFailed(`${relative(built.dir, page)} is not marked as release ${built.generation} alone`);
		}
	}
}

/** The release live now, or 0 when nothing is. */
export async function live(root: string): Promise<number> {
	try {
		return Number((await readFile(join(root, 'current', 'generation.txt'), 'utf8')).trim()) || 0;
	} catch {
		return 0;
	}
}

async function locked<T>(root: string, work: () => Promise<T>): Promise<T> {
	const lock = join(root, '.promote.lock');
	const deadline = Date.now() + LOCK_WAIT_MS;
	for (;;) {
		try {
			await mkdir(lock);
			break;
		} catch (error) {
			if ((error as NodeJS.ErrnoException).code !== 'EEXIST' || Date.now() > deadline) throw error;
			await new Promise((resume) => setTimeout(resume, 50));
		}
	}
	try {
		return await work();
	} finally {
		await rm(lock, { recursive: true, force: true });
	}
}

/** Verifies `built` and makes it the live site, unless a newer release is already live. */
export async function promote(root: string, built: Built): Promise<Promoted> {
	await verify(built);
	return locked(root, async () => {
		const current = await live(root);
		if (built.generation < current) return { promoted: false, reason: 'superseded', live: current };
		const next = join(root, `current.next-${process.pid}`);
		await rm(next, { force: true });
		await symlink(relative(root, built.dir), next);
		await rename(next, join(root, 'current'));
		return { promoted: true, generation: built.generation };
	});
}

/** The folder the live link points at, for tests and cleanup. */
export async function liveDir(root: string): Promise<string | null> {
	try {
		return join(root, await readlink(join(root, 'current')));
	} catch {
		return null;
	}
}
