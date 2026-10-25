import { defineConfig, devices } from '@playwright/test';

const PORT = 58201;

export default defineConfig({
	testDir: 'tests',
	fullyParallel: true,
	retries: 0,
	forbidOnly: Boolean(process.env['CI']),
	reporter: [['list']],
	use: {
		baseURL: `http://127.0.0.1:${PORT}`,
		trace: 'retain-on-failure'
	},
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }],
	webServer: {
		command: `npm run preview -- --port ${PORT}`,
		url: `http://127.0.0.1:${PORT}`,
		reuseExistingServer: false,
		timeout: 30_000
	}
});
