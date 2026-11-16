// Setup codes and sign-up through the testbed (step 3.7, D-061), against the real server. This
// suite's installation was bootstrapped by scripts/migrate.sh, so every code is refused with 409
// `already_set_up`, before any rate limit or comparison; signed out, presenting one is 401; and the
// create-account link redirects to the Studio front door that holds the password card (D-146). Making the first
// administrator with a code printed by a fresh server is in crates/colophon-server/tests/setup.rs
// and the Studio's journey.

import { expect, test } from '@playwright/test';
import { signIn } from './sign-in.ts';

test('a set-up installation refuses every setup code, and sign-up goes to the Studio front door', async ({ page, browser }) => {
	await page.goto('/setup');
	await expect(page.getByTestId('installation')).toContainText('Not signed in (HTTP 401)');
	await page.getByLabel('Setup code').fill('AAAAA-AAAAA-AAAAA-AAAAA');
	await page.getByRole('button', { name: 'Present the code' }).click();
	await expect(page.getByTestId('setup-outcome')).toHaveText('401 unauthorized');

	await signIn(page, 'dev-editor');
	await page.goto('/setup');
	await expect(page.getByTestId('installation')).toHaveText('This installation has an administrator; you are not an administrator.');
	for (const code of ['AAAAA-AAAAA-AAAAA-AAAAA', 'not a code', '']) {
		await page.getByLabel('Setup code').fill(code);
		await page.getByRole('button', { name: 'Present the code' }).click();
		await expect(page.getByTestId('setup-outcome')).toHaveText('409 conflict already_set_up');
	}
	// Six refusals later, still 409, not 429: nothing was counted once someone is an administrator.
	for (let attempt = 0; attempt < 3; attempt += 1) {
		await page.getByRole('button', { name: 'Present the code' }).click();
		await expect(page.getByTestId('setup-outcome')).toHaveText('409 conflict already_set_up');
	}
	await expect(page.getByTestId('installation')).toContainText('not an administrator');

	// With no identity provider, /auth/login sends the visitor to the Studio's front door, which
	// holds the password card (D-146). The testbed's server serves no Studio build (CI runs this
	// suite before the Studio is built), so the redirect is what is checked here; the card itself
	// is the Studio suite's (apps/studio/tests/server/journey.spec.ts).
	const visitor = await (await browser.newContext()).newPage();
	await visitor.goto('/setup');
	await visitor.getByTestId('create-account').click();
	await expect(visitor).toHaveURL(/\/studio\/\?return_to=%2Fme$/);
});
