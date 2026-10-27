import { summaries } from '#lib/server/blog.ts';
import type { PageServerLoad } from './$types';

export const load: PageServerLoad = () => ({ posts: summaries() });
