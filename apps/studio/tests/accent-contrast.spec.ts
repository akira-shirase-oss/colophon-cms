// Muted text on an accent ground (D-210): in dark mode --accent and --sidebar-accent are a strong blue that
// --muted-foreground can't be read on (2.60:1), so whatever turns a container to the accent must turn its muted
// text too. The project switcher is the case that exists: its slug is muted, and the button turns
// --sidebar-accent when hovered or open. axe measures the slug on the real ground, in both themes.

import AxeBuilder from '@axe-core/playwright';
import { expect, type Page, test } from '@playwright/test';
import { PROJECT, serve, world } from './site-preview-world.ts';

async function contrastViolations(page: Page) {
	const results = await new AxeBuilder({ page }).withRules(['color-contrast']).include('[data-sidebar="sidebar"]').analyze();
	return results.violations.flatMap((violation) => violation.nodes.map((node) => node.target.join(' ')));
}

for (const scheme of ['dark', 'light'] as const) {
	test(`the project switcher's muted slug reads on its accent ground when open and hovered: ${scheme}`, async ({ page, baseURL }) => {
		await page.setViewportSize({ width: 1440, height: 900 });
		await page.emulateMedia({ colorScheme: scheme, reducedMotion: 'reduce' });
		await serve(page, world(), new URL(baseURL ?? '').origin);
		await page.goto(`/studio/projects/${PROJECT}`);
		const switcher = page.getByTestId('project-name').locator('xpath=ancestor::button[1]');
		await expect(switcher).toBeVisible();

		await switcher.hover();
		expect(await contrastViolations(page), 'hovered').toEqual([]);

		await switcher.click();
		await expect(switcher).toHaveAttribute('data-state', 'open');
		expect(await contrastViolations(page), 'open').toEqual([]);
	});
}
