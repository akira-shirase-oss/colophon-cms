import { defineConfig, devices } from '@playwright/test';

const PORT = 58311;

// The built site, served as a static host would serve it, with the Pagefind index the build wrote.
// `npm run test` builds it first, so the tests always check the current source.
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
		url: `http://127.0.0.1:${PORT}/`,
		reuseExistingServer: false,
		timeout: 30_000
	}
});
