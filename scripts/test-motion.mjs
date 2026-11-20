// Light animations only: nothing in the Studio or its kit moves forever, except the few things
// below that show work in progress while it runs, each named with its reason. Skeletons are static
// (a loading shape that pulses forever is an infinite effect). A new pulse, ping, bounce, spin or
// `infinite` animation fails here until it is listed with why it may move, and an entry that no
// longer matches fails too, so the list stays exact. The kit's reduced-motion rule must stop them
// all: one pass, not an infinite loop sampled at 0.01ms.
//
//   node --test scripts/test-motion.mjs
import assert from 'node:assert/strict';
import { readdirSync, readFileSync } from 'node:fs';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const ROOTS = ['apps/studio/src', 'packages/ui/src'];
const MOVES_FOREVER = /\banimate-(?:pulse|ping|bounce|spin)\b|\binfinite\b/g;

// Work a person just started, shown only while it runs: [file, what moves, how many, why].
const SPINNER = 'a spinner: shown only while the work it names runs';
const EXCEPTIONS = [
	['apps/studio/src/lib/EntryEditor.svelte', 'animate-spin', 1, `${SPINNER} (saving)`],
	['apps/studio/src/lib/ChangesetPreview.svelte', 'animate-spin', 1, `${SPINNER} (opening the preview)`],
	['apps/studio/src/lib/PreviewPane.svelte', 'animate-spin', 2, `${SPINNER} (opening, refreshing the preview)`],
	['packages/ui/src/lib/components/ui/spinner/spinner.svelte', 'animate-spin', 1, `${SPINNER} (the kit's Spinner)`],
	['packages/ui/src/lib/components/ui/sonner/sonner.svelte', 'animate-spin', 1, `${SPINNER} (a toast's loading state)`]
];
// Not here on purpose: a status dot. A delivery or job can stay pending or running for minutes (its
// retries run up to an hour), so a dot that breathes while it waits is endless motion on an idle
// screen. Its colour and the words beside it carry the state.

function* files(dir) {
	for (const entry of readdirSync(path.join(repository, dir), { withFileTypes: true })) {
		const relative = path.join(dir, entry.name);
		if (entry.isDirectory()) yield* files(relative);
		else if (/\.(svelte|ts|css|js)$/.test(entry.name)) yield relative;
	}
}

function found() {
	const counts = new Map();
	for (const root of ROOTS) {
		for (const file of files(root)) {
			for (const match of readFileSync(path.join(repository, file), 'utf8').matchAll(MOVES_FOREVER)) {
				const key = `${file} ${match[0]}`;
				counts.set(key, (counts.get(key) ?? 0) + 1);
			}
		}
	}
	return counts;
}

test('nothing moves forever except the listed work-in-progress indicators', () => {
	const counts = found();
	const allowed = new Map(EXCEPTIONS.map(([file, what, count]) => [`${file} ${what}`, count]));
	const unlisted = [...counts].filter(([key, count]) => count > (allowed.get(key) ?? 0)).map(([key, count]) => `${key} ×${count}`);
	assert.deepEqual(unlisted, [], 'an infinite animation outside the exceptions: make it one-time, or list it with its reason');
	const stale = [...allowed].filter(([key, count]) => (counts.get(key) ?? 0) < count).map(([key]) => key);
	assert.deepEqual(stale, [], 'an exception no longer matches: remove it, so the list stays exact');
});

test('skeletons are static', () => {
	const skeleton = readFileSync(path.join(repository, 'packages/ui/src/lib/components/ui/skeleton/skeleton.svelte'), 'utf8');
	assert.doesNotMatch(skeleton, /animate-/);
});

test('reduced motion stops even the exceptions: one pass, not a loop', () => {
	const styles = readFileSync(path.join(repository, 'packages/ui/src/lib/styles.css'), 'utf8');
	const block = styles.slice(styles.indexOf('@media (prefers-reduced-motion: reduce)'));
	assert.match(block.slice(0, 600), /animation-iteration-count:\s*1\s*!important/);
});
