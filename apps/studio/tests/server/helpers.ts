// Shared steps for the Studio tests against a real server.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page } from '@playwright/test';

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
