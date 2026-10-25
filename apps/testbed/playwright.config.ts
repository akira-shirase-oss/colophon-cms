import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig, devices } from '@playwright/test';

// Playwright starts its own colophon-server (release build, local stack) and a preview of the
// built testbed, on ports of their own so they never collide with a dev server.
const SERVER_PORT = 58091;
const TESTBED_PORT = 58101;
const repo = fileURLToPath(new URL('../../', import.meta.url));

/** deploy/local.env, the single source of the server's local settings. */
function localEnv(): Record<string, string> {
	const env: Record<string, string> = {};
	for (const line of readFileSync(`${repo}deploy/local.env`, 'utf8').split('\n')) {
		const match = /^([A-Z_]+)=(.*)$/.exec(line.trim());
		if (match?.[1] !== undefined && match[2] !== undefined) env[match[1]] = match[2];
	}
	return env;
}

export default defineConfig({
	testDir: 'tests',
	// The outage test stops shared services, so tests run one at a time.
	workers: 1,
	fullyParallel: false,
	retries: 0,
	forbidOnly: Boolean(process.env['CI']),
	reporter: [['list']],
	timeout: 60_000,
	use: {
		baseURL: `http://127.0.0.1:${TESTBED_PORT}`,
		trace: 'retain-on-failure'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: [
		{
			command: `${repo}target/release/colophon-server`,
			env: {
				...localEnv(),
				COLOPHON_LISTEN_ADDR: `127.0.0.1:${SERVER_PORT}`,
				COLOPHON_LOG_FORMAT: 'json',
				COLOPHON_PROBE_INTERVAL_MS: '250',
				COLOPHON_PROBE_TIMEOUT_MS: '200'
			},
			url: `http://127.0.0.1:${SERVER_PORT}/livez`,
			reuseExistingServer: false,
			timeout: 30_000
		},
		{
			command: `npm run preview -- --port ${TESTBED_PORT}`,
			env: { COLOPHON_SERVER: `http://127.0.0.1:${SERVER_PORT}` },
			url: `http://127.0.0.1:${TESTBED_PORT}`,
			reuseExistingServer: false,
			timeout: 30_000
		}
	]
});
