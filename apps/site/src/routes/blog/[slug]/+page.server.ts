import { error } from '@sveltejs/kit';
import { posts, summaries } from '#lib/server/blog.ts';
import type { EntryGenerator, PageServerLoad } from './$types';

export const entries: EntryGenerator = () => posts.map((post) => ({ slug: post.slug }));

export const load: PageServerLoad = ({ params }) => {
	const index = posts.findIndex((post) => post.slug === params.slug);
	const post = posts[index];
	if (!post) error(404, 'No such post');
	const all = summaries();
	// Newest first: the next index is the older post.
	const older = all[index + 1] ?? null;
	const newer = all[index - 1] ?? null;
	// "You might also like": the posts on either side, then the newest one not already shown.
	const related = [older, newer, ...all.filter((other) => other.slug !== post.slug && other.slug !== older?.slug && other.slug !== newer?.slug)]
		.filter((other) => other !== null)
		.slice(0, 3);
	return { post, older, newer, related };
};
