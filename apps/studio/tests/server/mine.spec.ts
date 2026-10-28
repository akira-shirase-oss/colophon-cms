// "Only mine" on a collection's list (gap audit, 2026-10-28): the list narrows to the entries the
// signed-in person started, asked of the server (`created_by`), so it holds beyond the first page
// and together with a search; with none of theirs it says so and offers everyone's back; a role
// that reads only its own entries already sees nothing else, so it isn't offered the switch.

import { expect, test } from '@playwright/test';
import { api, internalTerms, member, newEntry, newProject, signIn, violations } from './helpers.ts';

test('a list narrows to the entries one started, on the server, and a role that reads only its own has no switch', async ({ page, browser }) => {
	await signIn(page);
	const project = await newProject(page);
	const role = await api(page, 'POST', `/api/v1/projects/${project}/roles`, {
		key: 'writer',
		name: 'Writer',
		permissions: [
			{ action: 'content.read', subject: 'articles', condition: 'own' },
			{ action: 'content.create', subject: 'articles' },
			{ action: 'content.update', subject: 'articles', condition: 'own' }
		]
	});
	expect(role.status, JSON.stringify(role.body)).toBe(201);
	const writer = await member(page, browser, project, 'writer');
	await newEntry(writer, project, 'The writer’s piece');

	// The owner has started nothing yet: "Only mine" says so, and gives everyone's back.
	await page.goto(`/studio/projects/${project}/articles`);
	const rows = page.getByTestId('entries');
	const onlyMine = page.getByTestId('only-mine');
	await expect(rows).toContainText('The writer’s piece');
	await expect(onlyMine).toHaveAttribute('aria-pressed', 'false');
	const me = (await api(page, 'GET', '/me')).body?.['data'] as { id: string };
	const asked = page.waitForRequest((request) => request.url().includes('/collections/articles/entries?') && request.url().includes(`created_by=${me.id}`));
	await onlyMine.click();
	await asked;
	await expect(onlyMine).toHaveAttribute('aria-pressed', 'true');
	await expect(rows).toContainText("You haven't started any here yet.");
	await expect(rows).not.toContainText('The writer’s piece');
	await rows.getByRole('button', { name: "Show everyone's" }).click();
	await expect(onlyMine).toHaveAttribute('aria-pressed', 'false');
	await expect(rows).toContainText('The writer’s piece');

	// With one of their own, "Only mine" keeps just that, and the counts follow.
	await newEntry(page, project, 'The owner’s piece');
	await page.reload();
	await expect(rows).toContainText('The owner’s piece');
	await expect(rows).toContainText('The writer’s piece');
	await expect(page.getByRole('tab', { name: /Drafts/ })).toContainText('2');
	await onlyMine.click();
	await expect(rows).not.toContainText('The writer’s piece');
	await expect(rows).toContainText('The owner’s piece');
	await expect(page.getByRole('tab', { name: /Drafts/ })).toContainText('1');
	expect(await internalTerms(page)).toEqual([]);
	expect(await violations(page)).toEqual([]);

	// Together with a search: only their titles that match.
	await page.getByTestId('entry-search').fill('writer');
	await expect(rows).toContainText('None of your titles match “writer”.');
	await page.getByTestId('entry-search').fill('owner');
	await expect(rows).toContainText('The owner’s piece');
	await page.getByTestId('entry-search').fill('');

	// Off again: everyone's.
	await onlyMine.click();
	await expect(rows).toContainText('The writer’s piece');
	await expect(rows).toContainText('The owner’s piece');

	// The writer reads only their own, so their list is already "only mine": no switch.
	await writer.goto(`/studio/projects/${project}/articles`);
	await expect(writer.getByTestId('entries')).toContainText('The writer’s piece');
	await expect(writer.getByTestId('entries')).not.toContainText('The owner’s piece');
	await expect(writer.getByTestId('only-mine')).toHaveCount(0);

	// On a phone, the switch fits beside the filters without scrolling sideways.
	await page.setViewportSize({ width: 390, height: 844 });
	await expect(onlyMine).toBeVisible();
	expect(await page.evaluate(() => document.documentElement.scrollWidth - window.innerWidth), 'the list scrolls sideways on a phone').toBeLessThanOrEqual(0);
});
