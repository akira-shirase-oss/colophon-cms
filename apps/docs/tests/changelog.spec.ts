// The changelog page shows CHANGELOG.md, the only place an entry is written: every section (a
// milestone, or Unreleased) and every entry, in the file's order, each with its anchor; the Atom
// feed has every merged step; each milestone's release notes carry its section; every range of
// migrations has an upgrade guide.

import { readFileSync } from 'node:fs';
import { join } from 'node:path';
import { expect, test } from '@playwright/test';
import { builtPages, docs, REPO } from './site.ts';

const source = readFileSync(join(REPO, 'CHANGELOG.md'), 'utf8');
const slug = (text: string) =>
	text
		.toLowerCase()
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '');

/** Sections and entries as the file has them, with the anchors both parsers make. */
function fromFile() {
	const sections: { title: string; id: string; entries: { heading: string; title: string; id: string; merged: boolean }[] }[] = [];
	let merged = false;
	for (const line of source.split('\n')) {
		if (line.startsWith('## ')) sections.push({ title: line.slice(3).trim(), id: slug(line.slice(3).trim()), entries: [] });
		else if (line.startsWith('### ')) {
			const heading = line.slice(4).trim();
			const [step = '', ...rest] = heading.split(' · ');
			sections.at(-1)?.entries.push({ heading, title: rest.join(' · '), id: `step-${slug(step)}`, merged: false });
			merged = false;
		} else if (line.startsWith('- **Merged:**') && !merged) {
			const entry = sections.at(-1)?.entries.at(-1);
			if (entry) entry.merged = true;
			merged = true;
		}
	}
	const all = sections.flatMap((section) => section.entries);
	for (const entry of all) {
		if (all.filter((other) => other.id === entry.id).length > 1) entry.id = `${entry.id}-${slug(entry.title)}`;
	}
	return sections;
}

const sections = fromFile();
const entries = sections.flatMap((section) => section.entries);

test('CHANGELOG.md is in the expected shape', () => {
	expect(sections[0]?.title).toBe('Unreleased');
	for (const milestone of ['P0', 'P1', 'P2', 'P3']) expect(sections.some((section) => section.title.startsWith(`${milestone} · `)), milestone).toBe(true);
	expect(entries.length).toBeGreaterThan(30);
	expect(new Set(entries.map((entry) => entry.id)).size, 'unique anchors').toBe(entries.length);
	// Keep a Changelog's groups, and nothing else at that level.
	const groups = [...source.matchAll(/^#### (.+)$/gm)].map((match) => match[1] ?? '');
	expect(groups.length).toBeGreaterThan(entries.length - 1);
	expect(groups.filter((group) => !['Added', 'Changed', 'Deprecated', 'Removed', 'Fixed', 'Security'].includes(group))).toEqual([]);
});

test('the changelog page lists every section and entry of CHANGELOG.md, in order, each with its anchor', async ({ page }) => {
	await page.goto(docs('/changelog/'), { waitUntil: 'networkidle' });
	const headings = await page.locator('article h2').evaluateAll((nodes) => nodes.map((node) => ({ id: node.id, text: node.textContent?.trim() ?? '' })));
	expect(headings).toEqual(sections.map((section) => ({ id: section.id, text: section.title })));
	await expect(page.getByTestId('changelog-entry')).toHaveCount(entries.length);
	const titles = await page.getByTestId('changelog-title').evaluateAll((nodes) => nodes.map((node) => ({ id: node.id, text: node.textContent?.trim() ?? '' })));
	expect(titles).toEqual(entries.map((entry) => ({ id: entry.id, text: entry.heading })));
	// Unreleased entries are labelled Building; merged ones Done, with their date.
	const unreleased = sections[0]?.entries.length ?? 0;
	const labels = await page.getByTestId('changelog-entry').evaluateAll((nodes) => nodes.map((node) => node.querySelector('[data-capability]')?.getAttribute('data-capability')));
	expect(labels.slice(0, unreleased).every((label) => label === 'building')).toBe(true);
	expect(labels.slice(unreleased).every((label) => label === 'done')).toBe(true);
});

test('a link to a step lands on its entry', async ({ page }) => {
	await page.goto(docs('/changelog/#step-2-2'), { waitUntil: 'networkidle' });
	await expect(page.locator('#step-2-2')).toBeInViewport();
	await expect(page.locator('#step-2-2')).toContainText('2.2 · ');
});

test('the Atom feed has every merged step, newest first, each linking to its anchor', async ({ request, page }) => {
	const response = await request.get(docs('/changelog/feed.xml'));
	expect(response.status()).toBe(200);
	expect(response.headers()['content-type']).toContain('application/atom+xml');
	const xml = await response.text();
	expect(xml).toContain('<feed xmlns="http://www.w3.org/2005/Atom">');
	const links = [...xml.matchAll(/<entry>[\s\S]*?<link rel="alternate" type="text\/html" href="[^"#]*#([^"]+)"\/>[\s\S]*?<updated>([^<]+)<\/updated>/g)].map((match) => ({
		id: match[1] ?? '',
		updated: match[2] ?? ''
	}));
	expect(links.map((link) => link.id).sort()).toEqual(entries.filter((entry) => entry.merged).map((entry) => entry.id).sort());
	const dates = links.map((link) => link.updated);
	expect(dates).toEqual([...dates].sort().reverse());
	// Every page advertises the feed.
	await page.goto(docs('/'), { waitUntil: 'networkidle' });
	await expect(page.locator('link[rel="alternate"][type="application/atom+xml"]')).toHaveAttribute('href', docs('/changelog/feed.xml'));
});

test("each milestone's release notes carry its section of the changelog", async ({ page }) => {
	for (const milestone of ['P0', 'P1', 'P2', 'P3']) {
		const section = sections.find((candidate) => candidate.title.startsWith(`${milestone} · `));
		await page.goto(docs(`/releases/${milestone.toLowerCase()}/`), { waitUntil: 'networkidle' });
		await expect(page.locator('h1')).toContainText(milestone);
		const embedded = page.locator(`[data-changelog-section="${milestone}"] .changelog-embed-title`);
		expect(await embedded.allTextContents(), milestone).toEqual((section?.entries ?? []).map((entry) => entry.heading));
	}
});

test('every range of migrations has an upgrade guide, and the release notes and guides are listed', async ({ page }) => {
	const pages = builtPages();
	for (const guide of ['p1', 'p2', 'p3', 'p3-roles-schedules']) expect(pages, guide).toContain(`/upgrading/${guide}/`);
	await page.goto(docs('/releases/'), { waitUntil: 'networkidle' });
	await expect(page.getByTestId('release-note')).toHaveCount(4);
	await page.goto(docs('/upgrading/'), { waitUntil: 'networkidle' });
	await expect(page.getByTestId('upgrade-guide')).toHaveCount(4);
});
