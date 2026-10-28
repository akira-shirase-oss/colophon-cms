// The first-run journey (step 3.7, D-059, D-061, D-082), on a brand-new installation: its own
// database, migrated and never bootstrapped, and its own server, whose setup link is read from its
// terminal as an operator would. A new person opens it, makes an account, becomes the administrator
// with the code the link filled in,
// creates a project from the blog starting point, writes and publishes the first article in one
// click, sees its receipt, and invites a teammate, who makes an account from the link, joins as
// an editor and asks for review; the owner approves and publishes. Every screen passes axe in
// light and dark and uses plain words; with STUDIO_SHOTS_DIR set, each step is photographed.

import { type ChildProcess, execFileSync, spawn } from 'node:child_process';
import { mkdirSync, readFileSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { type Browser, expect, type Page, test } from '@playwright/test';
import { internalTerms, violations } from './helpers.ts';

const repo = fileURLToPath(new URL('../../../../', import.meta.url));
// Its callback must be registered in deploy/keycloak's realm (58400 to 58405 are).
const PORT = Number(process.env['COLOPHON_JOURNEY_PORT'] ?? 58402);
const ORIGIN = `http://localhost:${PORT}`;
const SHOTS = process.env['STUDIO_SHOTS_DIR'];
const DATABASE = `colophon_journey_${Date.now().toString(36)}`;

function psql(sql: string) {
	execFileSync('docker', ['compose', '-f', `${repo}deploy/compose.yaml`, 'exec', '-T', 'postgres', 'psql', '-U', 'colophon', '-d', 'postgres', '-XAtqc', sql], {
		stdio: 'pipe'
	});
}

/** deploy/local.env, pointed at this journey's own database. */
function environment(): Record<string, string> {
	const env: Record<string, string> = {};
	for (const line of readFileSync(`${repo}deploy/local.env`, 'utf8').split('\n')) {
		const match = /^([A-Z_]+)=(.*)$/.exec(line.trim());
		if (match?.[1] === undefined || match[2] === undefined) continue;
		env[match[1]] = match[1].endsWith('DATABASE_URL') ? match[2].replace(/\/colophon$/, `/${DATABASE}`) : match[2];
	}
	return {
		...env,
		PATH: process.env['PATH'] ?? '',
		COLOPHON_LISTEN_ADDR: `127.0.0.1:${PORT}`,
		COLOPHON_PUBLIC_URL: ORIGIN,
		COLOPHON_STUDIO_DIR: `${repo}apps/studio/${process.env['STUDIO_BUILD_DIR'] ?? 'build'}`,
		COLOPHON_LOG_FORMAT: 'json'
	};
}

let server: ChildProcess | undefined;
let printed = '';

test.beforeAll(async () => {
	psql(`CREATE DATABASE ${DATABASE}`);
	execFileSync(`${repo}target/release/colophon`, ['admin', 'migrate'], { env: environment(), stdio: 'pipe' });
	server = spawn(`${repo}target/release/colophon-server`, [], { env: environment(), stdio: ['ignore', 'ignore', 'pipe'] });
	server.stderr?.on('data', (chunk: Buffer) => {
		printed += chunk.toString();
	});
	const deadline = Date.now() + 20_000;
	while (!/Setup code: /.test(printed)) {
		if (Date.now() > deadline) throw new Error(`the server printed no setup code:\n${printed}`);
		await new Promise((resolve) => setTimeout(resolve, 100));
	}
});

test.afterAll(async () => {
	// Stopped before its database goes, and waited for, so the port is free for what runs next.
	const running = server;
	if (running !== undefined && running.exitCode === null) {
		const stopped = new Promise((resolve) => running.once('exit', resolve));
		running.kill();
		await stopped;
	}
	psql(`DROP DATABASE IF EXISTS ${DATABASE} WITH (FORCE)`);
});

/** The code the server printed to its terminal, as the operator reads it. */
function setupCode(): string {
	const code = /Setup code: (\S+)/.exec(printed)?.[1];
	if (code === undefined) throw new Error('no setup code');
	return code;
}

/** The link the server printed beside it, which fills the code in (D-082). */
function setupLink(): string {
	const link = /(\S+#setup=\S+)/.exec(printed)?.[1];
	if (link === undefined) throw new Error('no setup link');
	return link;
}

let shot = 0;
/** One screen of the journey: axe and plain words in light and dark, and a photograph of each. */
async function screen(page: Page, name: string, { words = true }: { words?: boolean } = {}) {
	await page.waitForLoadState('networkidle');
	// From the top: the shell's header is sticky, so a page photographed scrolled looks broken.
	await page.evaluate(() => window.scrollTo(0, 0));
	shot += 1;
	for (const scheme of ['light', 'dark'] as const) {
		await page.emulateMedia({ colorScheme: scheme });
		await expect(page.locator('html')).toHaveClass(scheme === 'dark' ? /\bdark\b/ : /^(?!.*\bdark\b)/);
		expect(await violations(page), `${name}, ${scheme}`).toEqual([]);
		if (SHOTS !== undefined) {
			mkdirSync(SHOTS, { recursive: true });
			await page.screenshot({ path: join(SHOTS, `${String(shot).padStart(2, '0')}-${name}-${scheme}.png`), fullPage: true });
		}
	}
	await page.emulateMedia({ colorScheme: 'light' });
	if (words) expect(await internalTerms(page), `${name}: internal terms`).toEqual([]);
}

/** Keycloak's sign-up form, filled in for a new person; back in the Studio afterwards. */
async function signUp(page: Page, first: string, last: string): Promise<string> {
	const username = `${first.toLowerCase()}-${Date.now().toString(36)}`;
	await expect(page.locator('#kc-register-form')).toBeVisible();
	await page.locator('#username').fill(username);
	await page.locator('#email').fill(`${username}@example.test`);
	await page.locator('#firstName').fill(first);
	await page.locator('#lastName').fill(last);
	await page.locator('#password').fill('a-long-passphrase-1');
	await page.locator('#password-confirm').fill('a-long-passphrase-1');
	await page.locator('#kc-form-buttons input[type=submit], #kc-form-buttons button[type=submit]').click();
	return username;
}

async function fresh(browser: Browser): Promise<Page> {
	const context = await browser.newContext({ baseURL: ORIGIN });
	return context.newPage();
}

test('a new person sets up Colophon, publishes a first article, and brings in a teammate', async ({ browser }) => {
	test.setTimeout(300_000);
	const owner = await fresh(browser);
	let invitation = '';

	await test.step('a new installation greets a signed-out visitor', async () => {
		await owner.goto('/studio/');
		await expect(owner.getByTestId('signed-out')).toBeVisible();
		// Without the setup link, nothing signed out says whether anyone runs it yet.
		await expect(owner.getByTestId('new-installation')).toHaveCount(0);
		await screen(owner, 'welcome');
	});

	await test.step('the setup link from the terminal says this Colophon is new, with Create an account first', async () => {
		expect(setupLink()).toBe(`${ORIGIN}/studio/#setup=${setupCode()}`);
		await owner.goto(setupLink().slice(ORIGIN.length));
		await expect(owner.getByTestId('new-installation')).toContainText(
			"This Colophon is new. Create the first account; you'll become its administrator with the setup code the server printed."
		);
		await expect(owner.getByTestId('create-account')).toHaveClass(/\bbg-primary\b/);
		await expect(owner.getByTestId('sign-in')).not.toHaveClass(/\bbg-primary\b/);
		// The code leaves the address bar (and the history) and waits in this tab for the sign-up.
		await expect(owner).toHaveURL(`${ORIGIN}/studio/`);
		expect(await owner.evaluate(() => sessionStorage.getItem('colophon-setup-code'))).toBe(setupCode());
		await screen(owner, 'welcome-setup-link');
	});

	await test.step('they make an account at the identity provider', async () => {
		await owner.getByTestId('create-account').click();
		await expect(owner.locator('#kc-register-form')).toBeVisible();
		await screen(owner, 'sign-up', { words: false });
		await signUp(owner, 'Ada', 'Owner');
		await expect(owner.getByTestId('setup')).toBeVisible();
	});

	await test.step('nobody runs this Colophon yet: the code the link carried makes them its administrator', async () => {
		await expect(owner.getByRole('heading', { level: 1 })).toHaveText('Finish setting up Colophon');
		// The identity provider's redirect dropped the fragment; the tab kept the code.
		await expect(owner.getByLabel('Setup code')).toHaveValue(setupCode());
		await expect(owner.getByTestId('setup-from-link')).toHaveText('Code filled in from your link.');
		await screen(owner, 'setup');
		// Kept until used: a reload still has it.
		await owner.reload();
		await expect(owner.getByLabel('Setup code')).toHaveValue(setupCode());
		// A wrong code is refused, and typing over the link's code drops the note.
		await owner.getByLabel('Setup code').fill('AAAAA-AAAAA-AAAAA-AAAAA');
		await expect(owner.getByTestId('setup-from-link')).toHaveCount(0);
		await owner.getByRole('button', { name: 'Become the administrator' }).click();
		await expect(owner.getByTestId('setup-error')).toContainText("isn't right");
		// The code typed by hand, in lower case, works as well as the link's.
		await owner.getByLabel('Setup code').fill(setupCode().toLowerCase());
		await owner.getByRole('button', { name: 'Become the administrator' }).click();
		await expect(owner.getByTestId('first-project')).toBeVisible();
		await expect(owner.getByTestId('signed-in')).toContainText('administrator');
		// Used: forgotten.
		expect(await owner.evaluate(() => sessionStorage.getItem('colophon-setup-code'))).toBeNull();
		await screen(owner, 'first-project');
	});

	await test.step('they create a project from the blog starting point', async () => {
		await owner.getByLabel('Name', { exact: true }).fill('Field notes');
		await expect(owner.getByRole('textbox', { name: 'URL name' })).toHaveValue('field-notes');
		await expect(owner.getByRole('radio', { name: 'A small blog' })).toBeChecked();
		await owner.getByRole('button', { name: 'Create project' }).click();
		await expect(owner.getByTestId('first-run')).toBeVisible();
		await expect(owner.getByTestId('first-run-count')).toHaveText('0 of 5 done');
		await screen(owner, 'overview-checklist');
	});

	await test.step('they write the first article: its URL name comes from the title', async () => {
		await owner.getByRole('link', { name: 'Write: Write your first article' }).click();
		await expect(owner.getByRole('dialog')).toBeVisible();
		await owner.getByLabel('Title', { exact: true }).fill('Hello, readers');
		await screen(owner, 'new-article');
		await owner.getByRole('button', { name: 'Create', exact: true }).click();
		await expect(owner.getByRole('textbox', { name: 'URL name' })).toHaveValue('hello-readers');
		await owner.getByRole('textbox', { name: 'Body' }).click();
		await owner.keyboard.type('The first words on Field notes.');
		await expect(owner.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
		await screen(owner, 'editor');
	});

	await test.step('one click publishes it, with their approval recorded', async () => {
		await expect(owner.getByTestId('publish-button')).toHaveText('Publish');
		await owner.getByTestId('publish-button').click();
		await expect(owner.getByTestId('publish-outcome')).toContainText('Live as Release 1');
		await expect(owner.getByTestId('entry-state').first()).toHaveAttribute('data-state', 'live');
		await screen(owner, 'published');
	});

	await test.step('they see its receipt', async () => {
		await owner.getByTestId('publish-outcome').getByRole('link', { name: 'Release 1' }).click();
		await expect(owner.getByTestId('receipt-label')).toHaveText('Published in CMS');
		await expect(owner.getByTestId('receipt-explainer')).toContainText('Nothing checks your website yet');
		await screen(owner, 'receipt');
	});

	await test.step('a second article with the same title gets the next free URL name', async () => {
		await owner.goto(`${new URL(owner.url()).pathname.replace(/\/releases\/.*$/, '')}/articles?new=1`);
		await owner.getByLabel('Title', { exact: true }).fill('Hello, readers');
		await owner.getByRole('button', { name: 'Create', exact: true }).click();
		await expect(owner.getByRole('textbox', { name: 'URL name' })).toHaveValue('hello-readers-2');
		await expect(owner.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
	});

	await test.step('they invite a teammate', async () => {
		await owner.getByTestId('members-link').click();
		await expect(owner.getByTestId('members-alone')).toBeVisible();
		await screen(owner, 'members');
		await owner.getByLabel('Who is it for?').fill('Sam, writer');
		await owner.getByRole('button', { name: 'Create invitation link' }).click();
		invitation = await owner.getByRole('textbox', { name: 'Invitation link' }).inputValue();
		expect(invitation).toContain('/studio/invite#');
		await screen(owner, 'invited');
	});

	await test.step('the checklist follows what happened', async () => {
		await owner.getByRole('link', { name: 'Overview' }).click();
		await expect(owner.getByTestId('first-run-count')).toHaveText('4 of 5 done');
		for (const [item, done] of [
			['write', 'true'],
			['publish', 'true'],
			['connect', 'false'],
			['invite', 'true'],
			['receipt', 'true']
		] as const) {
			await expect(owner.locator(`[data-item=${item}]`)).toHaveAttribute('data-done', done);
		}
		await screen(owner, 'checklist-progress');
	});

	const teammate = await fresh(browser);
	await test.step('the teammate makes an account first, and learns they need an invitation', async () => {
		await teammate.goto('/studio/');
		await teammate.getByTestId('create-account').click();
		await signUp(teammate, 'Sam', 'Writer');
		await expect(teammate.getByTestId('not-administrator')).toContainText('invitation link');
		await screen(teammate, 'not-in-a-project');
	});

	await test.step('the link brings them in as an editor', async () => {
		await teammate.goto(invitation.replace(/^https?:\/\/[^/]+/, ''));
		await expect(teammate.getByTestId('invite-ask')).toContainText('Field notes');
		await screen(teammate, 'invitation');
		await teammate.getByRole('button', { name: 'Join', exact: true }).click();
		// Straight to their work there, with a note that says so (D-082).
		await expect(teammate.getByText('You joined Field notes as Editor', { exact: true })).toBeVisible();
		await expect(teammate.getByTestId('work-project')).toHaveText('Field notes');
		await screen(teammate, 'joined');
	});

	await test.step('an editor asks for review instead of publishing', async () => {
		await teammate.getByRole('link', { name: 'Open the project' }).click();
		await expect(teammate.getByTestId('project-name')).toHaveText('Field notes');
		await teammate.goto(`${new URL(teammate.url()).pathname}/articles?new=1`);
		await teammate.getByLabel('Title', { exact: true }).fill('A second voice');
		await teammate.getByRole('button', { name: 'Create', exact: true }).click();
		await teammate.getByRole('textbox', { name: 'Body' }).click();
		await teammate.keyboard.type('Written by the newest member.');
		await expect(teammate.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
		await expect(teammate.getByTestId('publish-button')).toHaveText('Ask for review');
		await teammate.getByTestId('publish-button').click();
		await expect(teammate.getByTestId('publish-outcome')).toContainText('Sent for review');
		await screen(teammate, 'asked-for-review');
	});

	await test.step('every project screen reads on a phone, and a reviewer finds Approve without scrolling (D-059, rule 7)', async () => {
		const base = new URL(owner.url()).pathname.replace(/\/$/, '');
		const entry = new URL(teammate.url()).pathname;
		const request = await teammate.getByTestId('publish-outcome').getByRole('link').getAttribute('href');
		expect(request).toBeTruthy();
		const phone = await (await browser.newContext({ baseURL: ORIGIN, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
		await phone.context().addCookies(await owner.context().cookies());
		const routes: [string, string][] = [
			['home', '/studio/'],
			['overview', base],
			['articles', `${base}/articles`],
			['editor', entry],
			['publish-requests', `${base}/changesets`],
			['review', request ?? ''],
			['releases', `${base}/releases`],
			['receipt', `${base}/releases/1`],
			['members', `${base}/members`],
			['settings', `${base}/settings`],
			['settings-roles', `${base}/settings/roles`],
			['settings-connections', `${base}/settings/connections`]
		];
		for (const [name, path] of routes) {
			await phone.goto(path);
			await phone.waitForLoadState('networkidle');
			const overflow = await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
			expect(overflow, `${path} (${name}) scrolls sideways on a phone`).toBeLessThanOrEqual(0);
			if (name === 'editor') {
				// The save and publish bar stays on screen below the fields.
				await expect(phone.getByTestId('save-status')).toHaveAttribute('data-status', 'saved');
				await expect(phone.getByTestId('publish-button')).toBeInViewport();
			}
			if (name === 'review') {
				await expect(phone.getByTestId('changeset-status')).toHaveAttribute('data-state', 'awaiting_review');
				await expect(phone.getByRole('button', { name: 'Approve this version' })).toBeInViewport();
			}
			// Settings is the developer area, where the internal names stay (D-059, rule 5).
			await screen(phone, `phone-${name}`, { words: !name.startsWith('settings') });
		}
		await phone.context().close();
	});

	await test.step('the owner approves it and publishes it', async () => {
		await owner.reload();
		await expect(owner.getByTestId('awaiting-count')).toHaveText('1');
		await owner.getByTestId('changesets-link').click();
		await screen(owner, 'publish-requests');
		await owner.getByTestId('changesets').getByRole('link', { name: 'A second voice' }).click();
		await expect(owner.getByTestId('changeset-status')).toHaveAttribute('data-state', 'awaiting_review');
		await expect(owner.getByTestId('diff')).toBeVisible();
		await screen(owner, 'review');
		await owner.getByRole('button', { name: 'Approve this version' }).click();
		await expect(owner.getByTestId('changeset-status')).toHaveAttribute('data-state', 'approved');
		await owner.getByRole('button', { name: 'Publish', exact: true }).click();
		await expect(owner.getByTestId('changeset-status')).toHaveAttribute('data-state', 'published');
		await expect(owner.getByTestId('release-link')).toHaveText('release 2');
		await screen(owner, 'approved-and-published');
	});

	await test.step('publish requests read on a phone, without sideways scrolling (D-059, rule 7)', async () => {
		const phone = await (await browser.newContext({ baseURL: ORIGIN, viewport: { width: 390, height: 844 }, isMobile: true, hasTouch: true })).newPage();
		const request = new URL(owner.url()).pathname;
		await phone.context().addCookies(await owner.context().cookies());
		for (const path of [request, request.replace(/\/changesets\/.*$/, '/changesets')]) {
			await phone.goto(path);
			await phone.waitForLoadState('networkidle');
			const overflow = await phone.evaluate(() => document.documentElement.scrollWidth - window.innerWidth);
			expect(overflow, `${path} scrolls sideways on a phone`).toBeLessThanOrEqual(0);
		}
		await phone.goto(request);
		// Opened directly, a published request's comparison once stayed "Loading…" for good.
		await expect(phone.getByTestId('diff')).toBeVisible();
		await screen(phone, 'phone-publish-request');
		await phone.context().close();
	});

	await test.step('hidden, the checklist stays hidden', async () => {
		await owner.getByRole('link', { name: 'Overview' }).click();
		await owner.getByRole('button', { name: 'Hide this list' }).click();
		await expect(owner.getByTestId('first-run')).toHaveCount(0);
		await owner.reload();
		await expect(owner.getByTestId('awaiting-count')).toHaveText('0');
		await expect(owner.getByTestId('first-run')).toHaveCount(0);
		await screen(owner, 'overview');
	});
});
