import { execFileSync } from 'node:child_process';
import { defineConfig, devices } from '@playwright/test';

// The preview server's port: STUDIO_E2E_PORT when set, otherwise a free one, so two checks on one
// machine (two worktrees, two heavy slots) never meet. The pick goes back into the environment, so
// the worker processes Playwright forks load this file and see the same port. --strictPort below
// makes a port taken in between fail the start instead of Vite moving on to the next one, where
// Playwright would test whatever else answers (scripts/test-free-ports.mjs).
function freePort(): string {
	return execFileSync(
		process.execPath,
		[
			'-e',
			"const s = require('node:net').createServer(); s.listen(0, '127.0.0.1', () => { process.stdout.write(String(s.address().port)); s.close(); });"
		],
		{ encoding: 'utf8' }
	);
}
process.env['STUDIO_E2E_PORT'] ||= freePort();
const PORT = Number(process.env['STUDIO_E2E_PORT']);

export default defineConfig({
	testDir: 'tests',
	// tests/server/ runs against a real colophon-server (playwright.server.config.ts).
	testIgnore: ['server/**', 'quickstart/**'],
	fullyParallel: true,
	retries: 0,
	forbidOnly: Boolean(process.env['CI']),
	reporter: [['list']],
	use: {
		baseURL: `http://127.0.0.1:${PORT}`,
		trace: 'retain-on-failure',
		// Checks run on settled pages; the theme-switch test opts back into motion.
		reducedMotion: 'reduce'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: {
		command: `npm run preview -- --port ${PORT} --strictPort`,
		url: `http://127.0.0.1:${PORT}/studio/playground`,
		reuseExistingServer: false,
		timeout: 30_000
	}
});
