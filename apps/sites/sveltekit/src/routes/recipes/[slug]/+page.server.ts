import { error } from '@sveltejs/kit';
import { richText, site } from '@colophon/site-kit';
import { buildRelease, entriesAt } from '#lib/server/colophon.ts';

// Which recipe pages to prerender: one per published recipe, at the build's release.
export async function entries() {
	const connection = site();
	if (connection === null) return [];
	const generation = await buildRelease(connection, 'recipes');
	return (await entriesAt(connection, 'recipes', generation)).map((r) => ({ slug: r.content.slug ?? r.id }));
}

export async function load({ params, parent }) {
	const { generation } = await parent();
	const connection = site();
	if (connection === null || generation === null) error(404, 'Not seeded');
	const recipe = (await entriesAt(connection, 'recipes', generation)).find((r) => (r.content.slug ?? r.id) === params.slug);
	if (!recipe) error(404, 'No such recipe');
	return {
		recipe: recipe.content,
		// The cook's notes are rich text, stored as JSON; the shared renderer turns them into HTML.
		notes: recipe.content.notes ? richText(connection, recipe.content.notes) : ''
	};
}
