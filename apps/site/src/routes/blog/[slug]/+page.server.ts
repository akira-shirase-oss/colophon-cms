import { error } from '@sveltejs/kit';
import { posts } from '#lib/server/blog.ts';
import type { EntryGenerator, PageServerLoad } from './$types';

export const entries: EntryGenerator = () => posts.map((post) => ({ slug: post.slug }));

export const load: PageServerLoad = ({ params }) => {
	const index = posts.findIndex((post) => post.slug === params.slug);
	const post = posts[index];
	if (!post) error(404, 'No such post');
	// Newest first: the next index is the older post.
	const neighbour = (other: (typeof posts)[number] | undefined) =>
		other ? { slug: other.slug, step: other.step, shortTitle: other.shortTitle } : null;
	return { post, older: neighbour(posts[index + 1]), newer: neighbour(posts[index - 1]) };
};
