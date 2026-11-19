import { site } from '@colophon/site-kit';
import { entriesAt } from '#lib/server/colophon.ts';

export async function load({ parent }) {
	const { generation } = await parent();
	const connection = site();
	if (connection === null || generation === null) return { recipes: [] };
	const recipes = await entriesAt(connection, 'recipes', generation);
	return {
		recipes: recipes
			.map((r) => ({ slug: r.content.slug ?? r.id, title: r.content.title, category: r.content.category, intro: r.content.intro }))
			.sort((a, b) => a.title.localeCompare(b.title))
	};
}
