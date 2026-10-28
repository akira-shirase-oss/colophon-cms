// Shared steps for the Studio tests against a real server.

import AxeBuilder from '@axe-core/playwright';
import { type Browser, expect, type Page } from '@playwright/test';

/** Completes the local Keycloak's sign-in form, wherever the page sent us to it. */
export async function keycloak(page: Page, user = 'dev-owner') {
	await page.locator('#username').fill(user);
	await page.locator('#password').fill('colophon-dev');
	await page.locator('#kc-login').click();
}

/** Signs in from the Studio's home page. */
export async function signIn(page: Page, user = 'dev-owner') {
	await page.goto('/studio/');
	await page.getByTestId('sign-in').click();
	await keycloak(page, user);
	await expect(page.getByTestId('signed-in')).toBeVisible();
}

/** A fresh project, created through the API as the signed-in page; its id. */
export async function newProject(page: Page): Promise<string> {
	return page.evaluate(async (slug) => {
		const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
		const response = await fetch('/api/v1/projects', {
			method: 'POST',
			headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf },
			body: JSON.stringify({ slug, name: 'Studio test' })
		});
		return ((await response.json()) as { data: { id: string } }).data.id;
	}, `st-${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 6)}`);
}

/** Accessibility problems on the page, one line per rule and element, so a failure says what. */
export async function violations(page: Page): Promise<string[]> {
	const results = await new AxeBuilder({ page }).analyze();
	return results.violations.flatMap((violation) =>
		violation.nodes.map((node) => `${violation.id}: ${node.target.join(' ')} (${node.failureSummary ?? ''})`)
	);
}

/**
 * Internal terms (D-059) in what the page shows or reads out: text, `aria-label`s, `title`s and
 * placeholders of visible elements, outside the Technical details drawer and what people wrote
 * themselves (`data-user-content`: a project called "Changesets testbed" is theirs to name). One
 * line per finding. Settings is the developer area, where these words may stay; don't call this
 * there.
 */
export async function internalTerms(page: Page): Promise<string[]> {
	const { INTERNAL_TERMS } = await import('../../src/lib/words.ts');
	return page.evaluate((source) => {
		const pattern = new RegExp(source, 'i');
		const found: string[] = [];
		const shown = (element: Element) =>
			element.closest('[data-technical], [data-user-content], script, style, template') === null &&
			element.checkVisibility({ visibilityProperty: true });
		const walker = document.createTreeWalker(document.body, NodeFilter.SHOW_TEXT);
		for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
			const parent = node.parentElement;
			const text = node.textContent ?? '';
			const match = pattern.exec(text);
			if (match !== null && parent !== null && shown(parent)) found.push(`“${match[0]}” in “${text.trim().slice(0, 100)}”`);
		}
		for (const element of document.body.querySelectorAll('[aria-label], [title], [placeholder]')) {
			for (const name of ['aria-label', 'title', 'placeholder']) {
				const value = element.getAttribute(name) ?? '';
				const match = pattern.exec(value);
				if (match !== null && shown(element)) found.push(`“${match[0]}” in ${name} “${value.slice(0, 100)}”`);
			}
		}
		return found;
	}, INTERNAL_TERMS.source);
}

/** An API call as the signed-in page (cookie and CSRF token): JSON in, status and JSON out. */
export async function api(
	page: Page,
	method: string,
	path: string,
	body?: unknown,
	headers: Record<string, string> = {}
): Promise<{ status: number; body: Record<string, any> | null }> {
	return page.evaluate(
		async ({ method, path, body, headers }) => {
			const csrf = document.cookie.split('; ').find((pair) => pair.startsWith('colophon_csrf='))?.slice('colophon_csrf='.length) ?? '';
			const response = await fetch(path, {
				method,
				headers: { 'Content-Type': 'application/json', 'X-CSRF-Token': csrf, ...headers },
				...(body === undefined ? {} : { body: JSON.stringify(body) })
			});
			return { status: response.status, body: (await response.json().catch(() => null)) as Record<string, any> | null };
		},
		{ method, path, body, headers }
	);
}

/** The production environment's API root for `project`. */
export const env = (project: string) => `/api/v1/projects/${project}/environments/production`;

/** A fresh idempotency key. */
export const key = () => crypto.randomUUID().replaceAll('-', '');

/** An entry made through the API; its id. `content` defaults to an article with an empty body. */
export async function newEntry(page: Page, project: string, title: string, collection = 'articles', content?: Record<string, unknown>): Promise<string> {
	const body = content ?? { title, body: { schema: 'colophon/rich-text', version: 1, doc: { type: 'doc', content: [{ type: 'paragraph' }] } } };
	const made = await api(page, 'POST', `${env(project)}/collections/${collection}/entries`, body, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	return made.body?.['data'].id as string;
}

/** Publishes `entries` through the API as the signed-in page: request, submit, approve, publish. */
export async function publishThroughApi(page: Page, project: string, title: string, entries: string[], unpublish: string[] = []): Promise<number> {
	const made = await api(page, 'POST', `${env(project)}/changesets`, { title, entries, unpublish }, { 'Idempotency-Key': key() });
	expect(made.status, JSON.stringify(made.body)).toBe(201);
	const set = made.body?.['data'] as { id: string; hash: string };
	const ifMatch = { 'If-Match': `"${set.hash}"` };
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/submit`, undefined, ifMatch)).status).toBe(200);
	expect((await api(page, 'POST', `${env(project)}/changesets/${set.id}/reviews`, { decision: 'approve' }, ifMatch)).status).toBe(200);
	const releases = await page.evaluate(async (path) => (await fetch(path)).headers.get('colophon-generation'), `${env(project)}/releases`);
	const published = await api(page, 'POST', `${env(project)}/changesets/${set.id}/publish`, { base_generation: Number(releases ?? '0') }, { ...ifMatch, 'Idempotency-Key': key() });
	expect(published.status, JSON.stringify(published.body)).toBe(201);
	return published.body?.['data'].generation as number;
}

/**
 * Someone else in the project: the owner (on `owner`, signed in) invites `user` with `role`; they
 * sign in in a browser of their own and join. Their page, on the project's Overview.
 */
export async function member(owner: Page, browser: Browser, project: string, role: string, user = 'dev-editor', viewport?: { width: number; height: number }): Promise<Page> {
	const invited = await api(owner, 'POST', `/api/v1/projects/${project}/invitations`, { role, label: `${user} as ${role}` });
	expect(invited.status, JSON.stringify(invited.body)).toBe(201);
	const token = String(invited.body?.['data'].link).split('#')[1];
	const context = await browser.newContext(viewport === undefined ? {} : { viewport, isMobile: true, hasTouch: true });
	const page = await context.newPage();
	await signIn(page, user);
	expect((await api(page, 'POST', '/api/v1/invitations/accept', { token })).status).toBe(200);
	await page.goto(`/studio/projects/${project}`);
	return page;
}

/** With STUDIO_SHOTS_DIR set, saves a full-page photograph of `page` there as `<name>.png`. */
export async function shot(page: Page, name: string) {
	const directory = process.env['STUDIO_SHOTS_DIR'];
	if (directory !== undefined && directory !== '') await page.screenshot({ path: `${directory}/${name}.png`, fullPage: true });
}
