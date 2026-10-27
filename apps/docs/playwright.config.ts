import { defineConfig, devices } from '@playwright/test';

// 58311 by default; COLOPHON_DOCS_TEST_PORT moves it when that port is taken.
const PORT = Number(process.env['COLOPHON_DOCS_TEST_PORT'] ?? 58311);

// The built site, served as the website's host will serve it (under /docs/), with the Pagefind index
// the build wrote. `npm run test` builds it first, so the tests always check the current source.
// Tests address pages through `docs()` in tests/site.ts, which adds the base path.
export default defineConfig({
	testDir: 'tests',
	fullyParallel: true,
	retries: 0,
	forbidOnly: Boolean(process.env['CI']),
	reporter: [['list']],
	use: {
		baseURL: `http://127.0.0.1:${PORT}`,
		trace: 'retain-on-failure',
		// Checks run on settled pages.
		reducedMotion: 'reduce'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: {
		command: `node scripts/serve.js ${PORT}`,
		url: `http://127.0.0.1:${PORT}/docs/`,
		reuseExistingServer: false,
		timeout: 30_000
	}
});
