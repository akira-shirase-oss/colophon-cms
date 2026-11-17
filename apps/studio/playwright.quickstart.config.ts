import { defineConfig, devices } from '@playwright/test';

// The Studio as a newcomer meets it from the quick start (D-172, D-173): the containers from
// deploy/quickstart/compose.yaml are already running, so there is no web server to start here.
// Run by scripts/quickstart.sh, which passes the address and the setup code it read from the logs.
const BASE = process.env['QUICKSTART_URL'] ?? 'http://localhost:8080';

export default defineConfig({
	testDir: 'tests/quickstart',
	workers: 1,
	fullyParallel: false,
	retries: 0,
	forbidOnly: Boolean(process.env['CI']),
	reporter: [['list']],
	timeout: 120_000,
	use: { baseURL: BASE, trace: 'retain-on-failure', reducedMotion: 'reduce' },
	projects: [{ name: 'chromium', use: { ...devices['Desktop Chrome'] } }]
});
