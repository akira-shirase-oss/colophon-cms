import { defineConfig, devices } from '@playwright/test';

const PORT = 58301;

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
	// The tests run against the prerendered site, so they build it first: `npm test` works from a
	// clean checkout, and what is tested is exactly what would be deployed.
	webServer: {
		command: `npm run build && npm run preview -- --port ${PORT}`,
		url: `http://127.0.0.1:${PORT}/`,
		reuseExistingServer: false,
		timeout: 180_000
	}
});
