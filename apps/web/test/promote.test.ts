import { mkdtemp, readFile, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { join } from 'node:path';
import { describe, expect, test } from 'vitest';

import { build } from '../src/build.ts';
import { VerifyFailed, live, liveDir, promote } from '../src/promote.ts';
import { api, item } from './colophon.ts';

const releases = new Map([1, 2, 3, 4, 5].map((generation) => [generation, [item(`a${generation}`, `Article ${generation}`, generation)]]));

async function built(root: string, generation: number) {
	return build({ api: 'http://colophon.test', project: 'p', environment: 'production', generation, root, fetch: api(releases).fetch });
}

describe('promotion', () => {
	test('goes live atomically, and an older build never replaces a newer one (X08)', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		expect(await live(root)).toBe(0);
		const second = await built(root, 2);
		expect(await promote(root, second)).toEqual({ promoted: true, generation: 2 });
		expect(await liveDir(root)).toBe(second.dir);
		const third = await built(root, 3);
		expect(await promote(root, third)).toEqual({ promoted: true, generation: 3 });
		const lateSecond = await built(root, 2);
		expect(await promote(root, lateSecond)).toEqual({ promoted: false, reason: 'superseded', live: 3 });
		expect(await liveDir(root)).toBe(third.dir);
		expect(await readFile(join(root, 'current', 'index.html'), 'utf8')).toContain('Article 3');
	});

	test('a build that fails its check never goes live (B1, B3)', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		await promote(root, await built(root, 1));
		const tampered = await built(root, 2);
		const page = join(tampered.dir, 'articles', 'a2', 'index.html');
		await writeFile(page, (await readFile(page, 'utf8')).replace('content="2"', 'content="1"'));
		await expect(promote(root, tampered)).rejects.toThrow(VerifyFailed);
		const doubled = await built(root, 2);
		const home = join(doubled.dir, 'index.html');
		await writeFile(home, `${await readFile(home, 'utf8')}<meta name="colophon-generation" content="2">`);
		await expect(promote(root, doubled)).rejects.toThrow('alone');
		expect(await live(root)).toBe(1);
	});

	test('promotions that race end on the newest release', async () => {
		const root = await mkdtemp(join(tmpdir(), 'site-'));
		const builds = await Promise.all([5, 4, 3].map((generation) => built(root, generation)));
		await Promise.all(builds.map((one) => promote(root, one)));
		expect(await live(root)).toBe(5);
	});
});
