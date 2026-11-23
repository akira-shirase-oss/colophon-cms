import { buildRelease, entriesAt } from '../lib/colophon.ts';

export default async function Home() {
	const pinned = buildRelease();
	if (!pinned) {
		return <p>No recipes yet. Seed the demo project first: <code>scripts/seed-demo-sites.sh</code> (apps/sites/README.md).</p>;
	}
	const recipes = (await entriesAt(pinned.site, 'recipes', pinned.generation)).sort((a, b) =>
		a.content.title.localeCompare(b.content.title)
	);
	return (
		<>
			<h1>Recipes</h1>
			<ul>
				{recipes.map((r) => (
					<li key={r.id}>
						<a href={`/recipes/${r.content.slug ?? r.id}/`}>{r.content.title}</a>
						{r.content.category ? <small> {r.content.category}</small> : null}
						{r.content.intro ? <p>{r.content.intro}</p> : null}
					</li>
				))}
			</ul>
		</>
	);
}
