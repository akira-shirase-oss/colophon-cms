import { changelog } from '#lib/server/changelog.ts';
import type { PageServerLoad } from './$types';

// The front page's "What changed lately": the newest merged steps and what is being built, from
// CHANGELOG.md at build time.
export const load: PageServerLoad = () => {
	const all = changelog().sections.flatMap((section) => section.entries);
	const merged = all.filter((entry) => entry.date !== null).sort((a, b) => (b.date ?? '').localeCompare(a.date ?? ''));
	const building = all.filter((entry) => entry.date === null);
	const pick = (entry: (typeof all)[number]) => ({ id: entry.id, step: entry.step, title: entry.title, date: entry.date, branch: entry.branch });
	return { latest: merged.slice(0, 4).map(pick), building: building.map(pick) };
};
