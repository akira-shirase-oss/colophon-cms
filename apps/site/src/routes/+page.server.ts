import why from '../../../../docs/WHY.md?raw';
import { posts, summaries } from '#lib/server/blog.ts';
import type { PageServerLoad } from './$types';

// The big quote: the question docs/WHY.md says Colophon exists to answer. The build fails if the
// file stops saying it.
const QUESTION = 'I published it, so why isn’t the site updated?';

// Sentences for the quote row, copied word for word from build-log posts. The build fails if one
// stops being in its post, so the row can't drift into words nobody wrote.
const QUOTES = [
	{ slug: 'checkpoint-2-4', text: 'Building the simulator found a real ordering bug in the previous step; it’s fixed, and neither the simulator nor the crash loop finds anything lost or doubled.' },
	{ slug: 'checkpoint-0-2', text: 'Docker said every service was healthy, so I broke one on purpose to see whether anyone would notice.' },
	{ slug: 'checkpoint-0-3', text: 'The millions found nothing. The frontend found a bug on day one.' },
	{ slug: 'checkpoint-1-4', text: 'It caught every bug I planted, agreed with the real database save for save, and found a real one in the Studio.' },
	{ slug: 'checkpoint-3-2', text: 'Until a CDN confirms, the release says the removal is still pending.' },
	{ slug: 'checkpoint-3-5', text: 'The Studio got a design of its own, twice: the first one looked like every other generated dashboard, so I rebuilt it from a reference I liked.' }
];

// Posts are rendered with smart punctuation, so compare with straight and curly apostrophes alike.
const plain = (text: string) => text.replaceAll('’', "'");

export const load: PageServerLoad = () => {
	if (!plain(why).includes(plain(QUESTION))) throw new Error(`the quote "${QUESTION}" is not in docs/WHY.md`);
	const quotes = QUOTES.map((quote) => {
		const post = posts.find((candidate) => candidate.slug === quote.slug);
		if (!post) throw new Error(`quote row: no post ${quote.slug}`);
		if (!plain(post.description).includes(plain(quote.text)) && !plain(post.html).includes(plain(quote.text))) {
			throw new Error(`quote row: "${quote.text}" is not in ${post.file}`);
		}
		return { ...quote, title: post.shortTitle, step: post.step };
	});
	return { posts: summaries().slice(0, 4), quotes, question: QUESTION };
};
