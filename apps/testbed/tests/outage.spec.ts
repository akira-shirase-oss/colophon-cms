// Stops real services of the local stack and checks that the testbed shows what happened.
// Services are restarted in `finally`, even when an assertion fails.

import { execFileSync } from 'node:child_process';
import { fileURLToPath } from 'node:url';
import { expect, test } from '@playwright/test';

const compose = fileURLToPath(new URL('../../../deploy/compose.yaml', import.meta.url));

function docker(...args: string[]) {
	execFileSync('docker', ['compose', '-f', compose, ...args], { stdio: 'ignore' });
}

test('a cache outage shows as degraded, then recovers to ready', async ({ page }) => {
	await page.goto('/');
	const status = page.getByTestId('status');
	await expect(status).toHaveAttribute('data-status', 'ready');
	try {
		docker('stop', 'valkey');
		await expect(status).toHaveAttribute('data-status', 'degraded', { timeout: 15_000 });
		await expect(page.getByTestId('http-status')).toHaveText('200');
		await expect(page.getByTestId('check-cache')).toHaveAttribute('data-state', 'down');
		await expect(page.getByTestId('transitions')).toContainText('ready → degraded');
	} finally {
		docker('start', 'valkey');
	}
	await expect(status).toHaveAttribute('data-status', 'ready', { timeout: 20_000 });
	await expect(page.getByTestId('transitions')).toContainText('degraded → ready');
});

test('a database outage shows as not ready with HTTP 503, then recovers', async ({ page }) => {
	await page.goto('/');
	const status = page.getByTestId('status');
	await expect(status).toHaveAttribute('data-status', 'ready');
	try {
		docker('stop', 'postgres');
		await expect(status).toHaveAttribute('data-status', 'not_ready', { timeout: 15_000 });
		await expect(page.getByTestId('http-status')).toHaveText('503');
		await expect(page.getByTestId('check-database')).toHaveAttribute('data-state', 'down');
	} finally {
		docker('start', 'postgres');
	}
	await expect(status).toHaveAttribute('data-status', 'ready', { timeout: 40_000 });
});
