// Self-serve sign-up's Studio pages (D-235) against answers the server would give: what
// /auth/providers publishes decides the sign-up page's state (closed: no form; invite-only: the
// invitation link, unless the link carries one; Turnstile only with a site key), a 422 lands under
// its field, a 429 says when to try again and holds the button, the usage page draws a meter per
// limit and none for an administrator's unbounded ones, and a display name is text, never markup.
// SCREENSHOTS=dir saves each state at a desktop and a phone width for review.

import { expect, type Page, test } from '@playwright/test';

type Registration = 'open' | 'invite' | 'closed';

const shots = process.env['SCREENSHOTS'];
const XSS = '<img src=x onerror="window.__xss=1">Ada';

function me(display_name: string, administrator = false) {
	return {
		id: 'u-1',
		issuer: 'colophon',
		subject: 'ada@example.test',
		display_name,
		administrator,
		installation: { has_administrator: true },
		projects: []
	};
}

interface World {
	registration?: Registration;
	siteKey?: string | null;
	signedIn?: ReturnType<typeof me> | null;
	hasAdministrator?: boolean;
	signUp?: { status: number; body: unknown; headers?: Record<string, string> };
	usage?: unknown;
	usageDelay?: Promise<void>;
}

/** Answers the Studio's calls from `world`; records sign-up bodies. */
async function serve(page: Page, world: World): Promise<unknown[]> {
	const sent: unknown[] = [];
	let signedIn = world.signedIn ?? null;
	await page.route('**/auth/providers', (route) =>
		route.fulfill({ json: { data: { sso: false, turnstile_site_key: world.siteKey ?? null, registration: world.registration ?? 'open' } } })
	);
	await page.route('**/api/v1/installation', (route) => route.fulfill({ json: { data: { has_administrator: world.hasAdministrator ?? true } } }));
	await page.route(
		(url) => url.pathname === '/me',
		(route) =>
			signedIn
				? route.fulfill({ json: { data: signedIn } })
				: route.fulfill({ status: 401, json: { error: { code: 'unauthorized', message: 'sign in first' } } })
	);
	await page.route('**/auth/signup', async (route) => {
		sent.push(route.request().postDataJSON());
		const answer = world.signUp ?? { status: 201, body: { data: { joined: null } } };
		if (answer.status < 300) signedIn = me((route.request().postDataJSON() as { display_name?: string }).display_name ?? 'Ada');
		await route.fulfill({ status: answer.status, json: answer.body, headers: answer.headers ?? {} });
	});
	await page.route('**/api/v1/me/usage', async (route) => {
		await world.usageDelay;
		await route.fulfill({ json: { data: world.usage } });
	});
	// Turnstile's script, stubbed: a widget that hands back a token at once.
	await page.route('https://challenges.cloudflare.com/**', (route) =>
		route.fulfill({
			contentType: 'text/javascript',
			body: `window.turnstile = { render(el, o) { el.textContent = 'Verified'; setTimeout(() => o.callback('tok-1')); return 'w1'; }, reset() {}, remove() {} };`
		})
	);
	return sent;
}

async function shoot(page: Page, name: string) {
	if (!shots) return;
	for (const [width, height] of [
		[1440, 900],
		[390, 844]
	] as const) {
		await page.setViewportSize({ width, height });
		await page.screenshot({ path: `${shots}/${name}-${width}.png`, fullPage: true });
	}
	await page.setViewportSize({ width: 1280, height: 720 });
}

test('closed registration shows no sign-up form and no way to one', async ({ page }) => {
	await serve(page, { registration: 'closed' });
	await page.goto('/studio/signup');
	await expect(page.getByTestId('signup-closed')).toBeVisible();
	await expect(page.locator('#auth-email')).toHaveCount(0);
	await shoot(page, 'signup-closed');
	await page.goto('/studio/signin');
	await expect(page.locator('#auth-email')).toBeVisible();
	await expect(page.getByTestId('to-signup')).toHaveCount(0);
	await shoot(page, 'signin');
});

test('invite-only asks for the link, and a link carrying a token signs up claiming it', async ({ page }) => {
	const sent = await serve(page, { registration: 'invite', signUp: { status: 201, body: { data: { joined: { project_id: 'p-1', role: 'editor' } } } } });
	await page.goto('/studio/signup');
	await expect(page.getByTestId('signup-invite-only')).toBeVisible();
	await expect(page.locator('#auth-email')).toHaveCount(0);
	await shoot(page, 'signup-invite-only');

	// An invitation link opens as a page of its own, as from an email.
	await page.goto('about:blank');
	await page.goto('/studio/signup#inv-token-1');
	await expect(page.getByTestId('signup-with-invitation')).toBeVisible();
	await expect(page).not.toHaveURL(/inv-token-1/);
	await shoot(page, 'signup-invite-link');
	await page.locator('#auth-email').fill('ada@example.test');
	await page.locator('#auth-password').fill('colophon-dev');
	await page.getByTestId('password-submit').click();
	await expect.poll(() => sent).toHaveLength(1);
	expect(sent[0]).toMatchObject({ email: 'ada@example.test', invitation: 'inv-token-1' });
});

test('the first account of a new invite-only installation may sign up without an invitation', async ({ page }) => {
	await serve(page, { registration: 'invite', hasAdministrator: false });
	await page.goto('/studio/signup');
	await expect(page.locator('#auth-email')).toBeVisible();
});

test("a 422 lands under its field in the Studio's words; editing clears it and the button stays usable", async ({ page }) => {
	const sent = await serve(page, {
		siteKey: '1x00000000000000000000AA',
		signUp: {
			status: 422,
			body: { error: { code: 'invalid_field', message: 'that is not an email address', details: { reason: 'invalid_email', pointer: '/email' } } }
		}
	});
	await page.goto('/studio/signup');
	await expect(page.getByTestId('turnstile')).toHaveText('Verified');
	await page.locator('#auth-email').fill('ada@localhost');
	await page.locator('#auth-password').fill('colophon-dev');
	await page.getByTestId('password-submit').click();
	await expect(page.getByTestId('field-error-email')).toHaveText('Enter an email address like name@example.com.');
	await expect(page.locator('#auth-email')).toHaveAttribute('aria-invalid', 'true');
	await expect(page.getByTestId('password-submit')).toBeEnabled();
	expect(sent[0]).toMatchObject({ turnstile_token: 'tok-1' });
	await shoot(page, 'signup-field-error');
	await page.locator('#auth-email').fill('ada@example.test');
	await expect(page.getByTestId('field-error-email')).toHaveCount(0);
	await expect(page.getByTestId('password-submit')).toBeEnabled();
});

test('password refusals read as one clear sentence each', async () => {
	const { fieldWords } = await import('../src/lib/shell/authErrors.ts');
	expect(fieldWords('weak_password', 'pick at least eight characters', 'short')).toBe('Use at least 8 characters.');
	expect(fieldWords('weak_password', 'keep it under 512 bytes', 'x'.repeat(513))).toBe('Use a password of 512 bytes or fewer.');
	expect(fieldWords('something_new', 'say it plainly')).toBe('Say it plainly');
});

test('sign-in with a wrong password says so plainly', async ({ page }) => {
	await serve(page, {});
	await page.route('**/auth/signin', (route) =>
		route.fulfill({ status: 400, json: { error: { code: 'sign_in_failed', message: 'sign-in failed', details: { reason: 'sign_in_failed' } } } })
	);
	await page.goto('/studio/signin');
	await page.locator('#auth-email').fill('ada@example.test');
	await page.locator('#auth-password').fill('not-the-password');
	await page.getByTestId('password-submit').click();
	await expect(page.getByTestId('auth-error')).toHaveText('That email and password did not match.');
	await shoot(page, 'signin-error');
});

test('sign-up shows a skeleton while the server says what it offers', async ({ page }) => {
	let answer = () => {};
	const later = new Promise<void>((resolve) => (answer = resolve));
	await serve(page, {});
	await page.route('**/auth/providers', async (route) => {
		await later;
		await route.fulfill({ json: { data: { sso: false, turnstile_site_key: null, registration: 'open' } } });
	});
	await page.goto('/studio/signup');
	await expect(page.getByTestId('signup-loading')).toBeVisible();
	await shoot(page, 'signup-loading');
	answer();
	await expect(page.locator('#auth-email')).toBeVisible();
});

test('no site key, no widget and no token', async ({ page }) => {
	const sent = await serve(page, {});
	await page.goto('/studio/signup');
	await page.locator('#auth-email').fill('ada@example.test');
	await page.locator('#auth-password').fill('colophon-dev');
	await expect(page.getByTestId('turnstile')).toHaveCount(0);
	await shoot(page, 'signup-empty');
	await page.getByTestId('password-submit').click();
	await expect.poll(() => sent).toHaveLength(1);
	expect(sent[0]).not.toHaveProperty('turnstile_token');
});

test('a 429 says when to try again and holds the button', async ({ page }) => {
	await page.clock.install({ time: new Date('2026-11-23T14:00:00') });
	await serve(page, {
		signUp: {
			status: 429,
			body: { error: { code: 'rate_limited', message: 'too many requests', details: { reason: 'rate_limited' } } },
			headers: { 'Retry-After': '1800' }
		}
	});
	await page.goto('/studio/signup');
	await page.locator('#auth-email').fill('ada@example.test');
	await page.locator('#auth-password').fill('colophon-dev');
	await page.getByTestId('password-submit').click();
	await expect(page.getByTestId('auth-error')).toContainText('Too many sign-ups from this network. Try again after');
	await expect(page.getByTestId('auth-error')).toContainText(/14:30|02:30 PM|2:30 PM/);
	await expect(page.getByTestId('password-submit')).toBeDisabled();
	await shoot(page, 'signup-429');
	await page.clock.runFor(1_800_000);
	await expect(page.getByTestId('password-submit')).toBeEnabled();
});

test('usage draws a meter per limit, flags a full one, and shows none for unbounded', async ({ page }) => {
	let arrive = () => {};
	const usageDelay = new Promise<void>((resolve) => (arrive = resolve));
	await serve(page, {
		signedIn: me('Ada'),
		usageDelay,
		usage: {
			projects: { used: 3, limit: 3 },
			entries: { used: 120, limit: 1000 },
			assets: { used: 4, limit: 500 },
			storage_bytes: { used: 52_400_000, limit: 1_000_000_000 }
		}
	});
	await page.goto('/studio/usage');
	await expect(page.getByTestId('usage-loading')).toBeVisible();
	await shoot(page, 'usage-loading');
	arrive();
	await expect(page.getByTestId('usage-projects')).toContainText('3 of 3');
	await expect(page.getByTestId('usage-projects')).toContainText('At the limit');
	await expect(page.getByTestId('usage-storage')).toContainText('52 MB of 1.0 GB');
	await expect(page.getByRole('progressbar')).toHaveCount(4);
	await shoot(page, 'usage');

	await page.unrouteAll({ behavior: 'ignoreErrors' });
	await serve(page, {
		signedIn: me('Admin', true),
		usage: {
			projects: { used: 12, limit: null },
			entries: { used: 0, limit: null },
			assets: { used: 0, limit: null },
			storage_bytes: { used: 0, limit: null }
		}
	});
	await page.goto('/studio/usage');
	await expect(page.getByTestId('usage-projects')).toContainText('No limit');
	await expect(page.getByRole('progressbar')).toHaveCount(0);
	await shoot(page, 'usage-admin');
});

test('usage signed out offers sign-in that comes back', async ({ page }) => {
	await serve(page, {});
	await page.goto('/studio/usage');
	await expect(page.getByTestId('usage-signed-out')).toBeVisible();
	await expect(page.getByRole('link', { name: 'Sign in' })).toHaveAttribute('href', /return_to=%2Fstudio%2Fusage/);
});

test('a display name is text, never markup', async ({ page }) => {
	await serve(page, {});
	await page.goto('/studio/signup');
	await page.locator('#auth-name').fill(XSS);
	await page.locator('#auth-email').fill('ada@example.test');
	await page.locator('#auth-password').fill('colophon-dev');
	await page.getByTestId('password-submit').click();
	await page.goto('/studio/usage');
	await page.getByTestId('account-menu').click();
	await expect(page.locator('[data-user-content]')).toHaveText(XSS);
	expect(await page.locator('[data-user-content] img').count()).toBe(0);
	expect(await page.evaluate(() => (window as { __xss?: number }).__xss)).toBeUndefined();
});
