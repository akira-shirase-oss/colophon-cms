// Two checks on one machine (two worktrees, or two heavy slots) must never meet on a port. The local
// gate (QUICK=1 scripts/check.sh) starts exactly one listener with a port of its own choosing: the
// Studio's e2e preview server (apps/studio/playwright.config.ts). This proves it picks a free port
// when STUDIO_E2E_PORT isn't set, even while the old fixed default (58201) is taken, keeps one port
// for every Playwright process, honours STUDIO_E2E_PORT, and fails on a taken port instead of
// letting Vite move to the next one (Playwright would then test whatever else answers there).
//
//   node --test scripts/test-free-ports.mjs
import assert from 'node:assert/strict';
import { execFileSync } from 'node:child_process';
import net from 'node:net';
import path from 'node:path';
import { test } from 'node:test';
import { fileURLToPath } from 'node:url';

const repository = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const config = path.join(repository, 'apps/studio/playwright.config.ts');

// What the config resolves to in a fresh Node process, as Playwright's main process sees it, plus
// what it left in the environment for the worker processes it forks.
function load(env) {
	const script = `const c = (await import(${JSON.stringify(config)})).default;
process.stdout.write(JSON.stringify({ baseURL: c.use.baseURL, url: c.webServer.url, command: c.webServer.command, env: process.env.STUDIO_E2E_PORT }));`;
	const out = execFileSync(process.execPath, ['--input-type=module', '-e', script], {
		cwd: path.join(repository, 'apps/studio'),
		env: { ...env, NODE_NO_WARNINGS: '1' },
		encoding: 'utf8'
	});
	return JSON.parse(out);
}

function portOf(url) {
	return Number(new URL(url).port);
}

function hold(port) {
	return new Promise((resolve, reject) => {
		const server = net.createServer();
		server.once('error', reject);
		server.listen(port, '127.0.0.1', () => resolve(server));
	});
}

const { STUDIO_E2E_PORT: _ignored, ...base } = process.env;

test('without STUDIO_E2E_PORT the e2e server gets a free port, never the taken old default', async (t) => {
	const old = await hold(58201).catch(() => null); // someone else may hold it already: same case
	t.after(() => old?.close());
	const seen = new Set();
	for (let i = 0; i < 3; i += 1) {
		const c = load(base);
		const port = portOf(c.baseURL);
		assert.notEqual(port, 58201, 'the fixed default is gone');
		assert.ok(port > 1024 && port < 65536, `a real port: ${port}`);
		// The main process and its workers agree: the pick is handed down through the environment.
		assert.equal(c.env, String(port));
		assert.equal(portOf(c.url), port);
		assert.match(c.command, new RegExp(`--port ${port}\\b`));
		seen.add(port);
	}
	// A taken port is never handed out (the picker binds it before answering).
	const held = await hold(0);
	t.after(() => held.close());
	for (let i = 0; i < 5; i += 1) {
		assert.notEqual(portOf(load(base).baseURL), held.address().port);
	}
	assert.ok(seen.size >= 1);
});

test('STUDIO_E2E_PORT is honoured exactly', () => {
	const c = load({ ...base, STUDIO_E2E_PORT: '58177' });
	assert.equal(portOf(c.baseURL), 58177);
	assert.equal(portOf(c.url), 58177);
	assert.match(c.command, /--port 58177\b/);
});

test('a taken port fails the start instead of drifting to the next one', () => {
	assert.match(load(base).command, /--strictPort\b/);
});
