import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { defineConfig, devices } from '@playwright/test';

// The Studio against a real colophon-server: the release build serves the built Studio at
// /studio on its own origin (D-037), with the local stack's Postgres and Keycloak behind it.
// Run by scripts/studio.sh.
const PORT = 58093;
const repo = fileURLToPath(new URL('../../', import.meta.url));

/** deploy/local.env, the single source of the server's local settings. With COLOPHON_TEST_DATABASE
 *  set, the database URLs name that database instead of `colophon` (scripts/local-env.sh). */
function localEnv(): Record<string, string> {
	const env: Record<string, string> = {};
	const database = process.env['COLOPHON_TEST_DATABASE'];
	for (const line of readFileSync(`${repo}deploy/local.env`, 'utf8').split('\n')) {
		const match = /^([A-Z_]+)=(.*)$/.exec(line.trim());
		if (match?.[1] === undefined || match[2] === undefined) continue;
		const named = database !== undefined && match[1].endsWith('DATABASE_URL') && match[2].endsWith('/colophon');
		env[match[1]] = named ? `${match[2].slice(0, -'colophon'.length)}${database}` : match[2];
	}
	return env;
}

export default defineConfig({
	testDir: 'tests/server',
	workers: 1,
	fullyParallel: false,
	retries: 0,
	forbidOnly: Boolean(process.env['CI']),
	reporter: [['list']],
	timeout: 60_000,
	use: {
		baseURL: `http://localhost:${PORT}`,
		trace: 'retain-on-failure',
		// Checks run on settled pages; the theme-switch test opts back into motion.
		reducedMotion: 'reduce'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: {
		command: `${repo}target/release/colophon-server`,
		env: {
			...localEnv(),
			COLOPHON_LISTEN_ADDR: `127.0.0.1:${PORT}`,
			COLOPHON_PUBLIC_URL: `http://localhost:${PORT}`,
			COLOPHON_STUDIO_DIR: `${repo}apps/studio/build`,
			COLOPHON_LOG_FORMAT: 'json'
		},
		// The server's log in the test output: a failed sign-in logs its reason at info.
		stdout: 'pipe',
		url: `http://127.0.0.1:${PORT}/livez`,
		reuseExistingServer: false,
		timeout: 30_000
	}
});
