// Device sign-in through the testbed, in a real browser: start, poll (pending), approve as the
// signed-in person, poll again for the token, and use the token with no cookie at all. A second
// poll finds nothing: the approval is exchanged exactly once.

import { expect, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

test('start a device sign-in, approve it, and use the token without a cookie', async ({ page }) => {
	await signIn(page, 'dev-owner');
	await page.goto('/device');
	await page.getByLabel('Terminal name').fill('testbed terminal');
	await page.getByRole('button', { name: 'Start device sign-in' }).click();
	await expect(page.getByTestId('code')).toHaveText(/^[BCDFGHJKLMNPQRSTVWXZ2-9]{4}-[BCDFGHJKLMNPQRSTVWXZ2-9]{4}$/);
	await expect(page.getByTestId('link')).toContainText('/studio/device?code=');

	await page.getByRole('button', { name: 'Poll' }).click();
	await expect(page.getByTestId('status')).toHaveText('pending');
	await page.getByRole('button', { name: 'Approve as me' }).click();
	await expect(page.getByTestId('status')).toHaveText('approved in the browser');
	await page.getByRole('button', { name: 'Poll' }).click();
	await expect(page.getByTestId('status')).toHaveText('approved');
	await expect(page.getByTestId('me')).toHaveText('HTTP 200 as Dev Owner');

	await page.getByRole('button', { name: 'Poll' }).click();
	await expect(page.getByTestId('status')).toHaveText('404 not_found');
});
