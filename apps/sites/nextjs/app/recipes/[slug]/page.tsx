import { notFound } from 'next/navigation';
import { buildRelease, entriesAt } from '../../../lib/colophon.ts';

// Only the recipes published at the build's release exist; any other path is a 404.
export const dynamicParams = false;

// Which recipe pages to export: one per published recipe, at the build's release. Before seeding there are none,
// and a static export needs at least one, so the build makes a single placeholder page.
export async function generateStaticParams() {
	const pinned = buildRelease();
	if (!pinned) return [{ slug: 'not-seeded' }];
	const recipes = await entriesAt(pinned.site, 'recipes', pinned.generation);
	return recipes.map((r) => ({ slug: r.content.slug ?? r.id }));
}

export default async function RecipePage({ params }: { params: Promise<{ slug: string }> }) {
	const { slug } = await params;
	const pinned = buildRelease();
	if (!pinned) return <p>Seed the demo project first.</p>;
	const recipe = (await entriesAt(pinned.site, 'recipes', pinned.generation)).find((r) => (r.content.slug ?? r.id) === slug);
	if (!recipe) notFound();
	const r = recipe.content;
	return (
		<article>
			<h1>{r.title}</h1>
			{r.intro ? <p>{r.intro}</p> : null}
			<p>
				{[r.category, r.difficulty, r.prep_minutes != null ? `prep ${r.prep_minutes} min` : null, r.cook_minutes != null ? `cook ${r.cook_minutes} min` : null, r.servings != null ? `serves ${r.servings}` : null]
					.filter(Boolean)
					.join(' · ')}
			</p>
			{r.ingredients?.length ? (
				<>
					<h2>Ingredients</h2>
					<ul>{r.ingredients.map((i, n) => <li key={n}>{i.amount ? `${i.amount} ` : ''}{i.item}</li>)}</ul>
				</>
			) : null}
			{r.steps?.length ? (
				<>
					<h2>Method</h2>
					<ol>{r.steps.map((step, n) => <li key={n}>{step}</li>)}</ol>
				</>
			) : null}
		</article>
	);
}
