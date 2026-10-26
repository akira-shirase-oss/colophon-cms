// Shared steps for the Studio tests against a real server.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page } from '@playwright/test';

/** Completes the local Keycloak's sign-in form, wherever the page sent us to it. */
export async function keycloak(page: Page, user = 'dev-editor') {
	await page.locator('#username').fill(user);
	await page.locator('#password').fill('colophon-dev');
	await page.locator('#kc-login').click();
}

/** Signs in from the Studio's home page. */
export async function signIn(page: Page, user = 'dev-editor') {
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
