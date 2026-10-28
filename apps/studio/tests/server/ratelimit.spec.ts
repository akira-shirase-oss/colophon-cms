// Rate limits (step S.2, D-076) in the Studio, against a real server of its own whose limit per
// session is tiny (one request a second after a burst), so the test can use it all up. An edit
// saved while the session is over its limit is turned away (429 with Retry-After); the editor says
// so plainly, keeps the edit on this device, waits as long as the server asked and sends the same
// save again, Idempotency-Key and all, and the edit is stored once.

import { type ChildProcess, spawn } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';
import { INTERNAL_TERMS } from '../../src/lib/words.ts';
import { api, env, internalTerms, newEntry, newProject, signIn, violations } from './helpers.ts';

const repo = fileURLToPath(new URL('../../../../', import.meta.url));
// Its own port, whose sign-in callback is registered in deploy/keycloak's realm (58400 to 58405).
const PORT = Number(process.env['COLOPHON_JOURNEY_PORT'] ?? 58402);
const ORIGIN = `http://localhost:${PORT}`;

/** deploy/local.env, on the test database, with a tiny limit per session and none per address
 *  (every local test shares 127.0.0.1). */
function environment(): Record<string, string> {
	const database = process.env['COLOPHON_TEST_DATABASE'];
	const values: Record<string, string> = {};
	for (const line of readFileSync(`${repo}deploy/local.env`, 'utf8').split('\n')) {
		const match = /^([A-Z_]+)=(.*)$/.exec(line.trim());
		if (match?.[1] === undefined || match[2] === undefined) continue;
		const named = database !== undefined && match[1].endsWith('DATABASE_URL') && match[2].endsWith('/colophon');
		values[match[1]] = named ? `${match[2].slice(0, -'colophon'.length)}${database}` : match[2];
	}
	return {
		...values,
		PATH: process.env['PATH'] ?? '',
		COLOPHON_LISTEN_ADDR: `127.0.0.1:${PORT}`,
		COLOPHON_PUBLIC_URL: ORIGIN,
		COLOPHON_STUDIO_DIR: `${repo}apps/studio/build`,
		COLOPHON_LOG_FORMAT: 'json',
		COLOPHON_RATE_LIMIT_PER_ADDRESS: 'off',
		COLOPHON_RATE_LIMIT_PER_CREDENTIAL: '1/40'
	};
}

let server: ChildProcess | undefined;
let printed = '';

const answers = () =>
	fetch(`http://127.0.0.1:${PORT}/livez`).then(
		(response) => response.ok,
		() => false
	);

test.beforeAll(async () => {
	// The port is shared with the first-run journey's server: wait until nothing answers on it.
	const free = Date.now() + 20_000;
	while (await answers()) {
		if (Date.now() > free) throw new Error(`something else is serving on ${PORT}`);
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
	server = spawn(`${repo}target/release/colophon-server`, [], { env: environment(), stdio: ['ignore', 'ignore', 'pipe'] });
	server.stderr?.on('data', (chunk: Buffer) => {
		printed += chunk.toString();
	});
	const deadline = Date.now() + 20_000;
	for (;;) {
		// Ours, not one still stopping on the port: a server that couldn't listen has exited.
		if (server.exitCode !== null) throw new Error(`the rate-limited server stopped:\n${printed}`);
		if ((await answers()) && server.exitCode === null) break;
		if (Date.now() > deadline) throw new Error(`the rate-limited server didn't start on ${PORT}:\n${printed}`);
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
});

/** Stops the server and waits until it has, so the port is free for whatever runs next. */
test.afterAll(async () => {
	const running = server;
	if (running === undefined || running.exitCode !== null) return;
	const stopped = new Promise((resolve) => running.once('exit', resolve));
	running.kill();
	await stopped;
});

test.use({ baseURL: ORIGIN });

test('an edit saved while the session is over its limit waits as asked, is kept meanwhile, and is stored once', async ({ page }) => {
	await signIn(page);
	const project = await newProject(page);
	const entry = await newEntry(page, project, 'Before the limit');
	await page.goto(`/studio/projects/${project}/articles/${entry}`);
	const status = page.getByTestId('save-status');
	await expect(status).toHaveAttribute('data-status', 'saved');
	await page.waitForLoadState('networkidle');

	// Every save of this entry: its key and the answer.
	const saves: { key: string | undefined; status: number }[] = [];
	page.on('response', (response) => {
		const request = response.request();
		if (request.method() === 'PUT' && request.url().endsWith(`/entries/${entry}`)) saves.push({ key: request.headers()['idempotency-key'], status: response.status() });
	});

	await page.getByLabel('Title', { exact: true }).fill('Saved after the limit');
	// Use up the session's limit, then save at once: the save is turned away.
	const answers = await page.evaluate(() => Promise.all(Array.from({ length: 60 }, () => fetch('/api/v1/me', { cache: 'no-store' }).then((response) => response.status))));
	expect(answers).toContain(429);
	await page.keyboard.press('ControlOrMeta+s');

	// The wait is a second or two: what the editor shows and keeps is read the moment it starts.
	const during = await page.waitForFunction((key) => {
		const shown = document.querySelector('[data-testid="save-status"]');
		return shown?.getAttribute('data-status') === 'retrying' ? { words: shown.textContent?.trim() ?? '', kept: localStorage.getItem(key) } : null;
	}, `colophon:unsaved:${project}:${entry}`);
	const { words, kept } = (await during.jsonValue()) as { words: string; kept: string | null };
	expect(words).toMatch(/^Colophon is busy for a moment; trying again in \d+ seconds?\. Your changes are kept on this device\.$/);
	expect(INTERNAL_TERMS.test(words)).toBe(false);
	expect(JSON.parse(kept ?? '{}').content).toMatchObject({ title: 'Saved after the limit' });

	// Once the wait is over, the same save goes again and is stored.
	await expect(status).toHaveAttribute('data-status', 'saved', { timeout: 20_000 });
	expect(saves[0]?.status).toBe(429);
	expect(saves.at(-1)?.status).toBe(200);
	expect(new Set(saves.map((save) => save.key)).size, JSON.stringify(saves)).toBe(1);
	// Read back once the session may read again (the test's own request is limited too).
	await expect
		.poll(async () => (await api(page, 'GET', `${env(project)}/collections/articles/entries/${entry}`)).body?.['data']?.content?.title, { timeout: 10_000 })
		.toBe('Saved after the limit');
	expect(await page.evaluate((key) => localStorage.getItem(key), `colophon:unsaved:${project}:${entry}`)).toBeNull();
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// On a phone, the editor's words fit.
	await page.setViewportSize({ width: 390, height: 844 });
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the editor scrolls sideways on a phone').toBeLessThanOrEqual(0);
});
